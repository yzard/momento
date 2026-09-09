package io.github.yzard.momento.core.cache

import java.io.File
import java.io.IOException
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import okio.Buffer
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class MediaCacheTest {
    @get:Rule val temporary = TemporaryFolder(File(requireNotNull(System.getProperty("java.io.tmpdir"))))
    private var clock = 1000L
    private var online = true
    private var namespace: String? = "account-a"
    private val url = "https://momento.example/api/v1/media/1/thumbnail"

    private fun client(cache: DiskMediaCache, network: (Request) -> Response): OkHttpClient = OkHttpClient.Builder()
        .addInterceptor(MediaCacheInterceptor(cache, { namespace }, { online }, { clock }))
        .addInterceptor(Interceptor { network(it.request()) }).build()
    private fun response(request: Request, content: String, code: Int, etag: String): Response = Response.Builder()
        .request(request).protocol(Protocol.HTTP_1_1).code(code).message("Test")
        .header("ETag", etag).body(content.toResponseBody("image/jpeg".toMediaType())).build()
    private fun read(client: OkHttpClient, target: String = url): String = client.newCall(Request.Builder().url(target).build()).execute().use { it.body!!.string() }
    private fun put(cache: DiskMediaCache, key: String, content: String) {
        val editor = cache.begin(key, content.length.toLong(), CachedMediaMetadata("image/jpeg", "tag", null, clock))!!
        editor.append(Buffer().writeUtf8(content), 0, content.length.toLong())
        editor.complete()
    }

    @Test fun cachesFreshAssetsAndReopensThemOfflineAfterRestart() {
        val directory = temporary.newFolder()
        val cache = DiskMediaCache(directory, 1024 * 1024)
        var requests = 0
        val first = client(cache) { requests++; response(it, "thumbnail", 200, "v1") }
        assertEquals("thumbnail", read(first))
        assertEquals("thumbnail", read(first))
        assertEquals(1, requests)
        online = false
        clock += MediaCacheInterceptor.FRESH_MILLIS + 1
        val reopened = client(DiskMediaCache(directory, 1024 * 1024)) { throw IOException("Offline") }
        assertEquals("thumbnail", read(reopened))
    }

    @Test fun validatesExpiredAssetsAndReplacesChangedBytes() {
        var requests = 0
        val cache = DiskMediaCache(temporary.newFolder(), 1024 * 1024)
        val client = client(cache) {
            requests++
            if (requests == 1) response(it, "old", 200, "v1")
            else {
                assertEquals("v1", it.header("If-None-Match"))
                if (requests == 2) response(it, "", 304, "v1") else response(it, "new", 200, "v2")
            }
        }
        assertEquals("old", read(client))
        clock += MediaCacheInterceptor.FRESH_MILLIS + 1
        assertEquals("old", read(client))
        assertEquals("old", read(client))
        assertEquals(2, requests)
        clock += MediaCacheInterceptor.FRESH_MILLIS + 1
        assertEquals("new", read(client))
        online = false
        assertEquals("new", read(client))
    }

    @Test fun fallsBackOnConnectionFailureButNotAccessRevocation() {
        val cache = DiskMediaCache(temporary.newFolder(), 1024 * 1024)
        assertEquals("cached", read(client(cache) { response(it, "cached", 200, "v1") }))
        clock += MediaCacheInterceptor.FRESH_MILLIS + 1
        assertEquals("cached", read(client(cache) { throw IOException("Disconnected") }))
        val revoked = client(cache) { response(it, "Forbidden", 403, "") }
        revoked.newCall(Request.Builder().url(url).build()).execute().use { assertEquals(403, it.code) }
        online = false
        assertThrows(IOException::class.java) { read(client(cache) { throw IOException("Offline") }) }
    }

    @Test fun doesNotShareCachedBytesAcrossAccountsOrServers() {
        val cache = DiskMediaCache(temporary.newFolder(), 1024 * 1024)
        assertEquals("private", read(client(cache) { response(it, "private", 200, "v1") }))
        online = false
        val offline = client(cache) { throw IOException("Offline") }
        assertThrows(IOException::class.java) { read(offline, url.replace("momento.example", "other.example")) }
        namespace = "account-b"
        assertThrows(IOException::class.java) { read(offline) }
        namespace = null
        assertThrows(IOException::class.java) { read(offline) }
    }

    @Test fun offlineReadSnapshotsAreKeyedByRequestAndMutationsAreNeverReplayed() {
        val cache = DiskMediaCache(temporary.newFolder(), 1024 * 1024)
        val client = client(cache) { response(it, "page-one", 200, "") }
        fun request(path: String, body: String) = Request.Builder().url("https://momento.example/api/v1/$path")
            .post(body.toRequestBody("application/json".toMediaType())).build()
        val page = request("timeline/list", "{\"cursor\":null}")
        client.newCall(page).execute().use { assertEquals("page-one", it.body!!.string()) }
        online = false
        val offline = client(cache) { throw IOException("Offline") }
        offline.newCall(page).execute().use { assertEquals("page-one", it.body!!.string()) }
        assertThrows(IOException::class.java) { offline.newCall(request("timeline/list", "{\"cursor\":\"next\"}")).execute() }
        assertThrows(IOException::class.java) { offline.newCall(request("media/delete", "{}")).execute() }
    }

    @Test fun shrinkingCacheEvictsLeastRecentlyUsedAndCancelsPendingWrites() {
        val directory = temporary.newFolder()
        val cache = DiskMediaCache(directory, 100_000)
        put(cache, "a", "a".repeat(20_000))
        put(cache, "b", "b".repeat(20_000))
        cache.snapshot("a", ++clock)!!.close()
        val pending = cache.begin("c", 100, CachedMediaMetadata(null, null, null, clock))!!
        cache.resize(25_000)
        pending.append(Buffer().writeUtf8("c".repeat(100)), 0, 100)
        assertNull(cache.snapshot("b", clock))
        assertNull(cache.snapshot("c", clock))
        assertNotNull(cache.snapshot("a", clock)?.also { it.close() })
        assertTrue(directory.listFiles()!!.sumOf { it.length() } <= 25_000)
    }

    @Test fun cancelledOrOversizedDownloadsDoNotPublishPartialEntries() {
        val directory = temporary.newFolder()
        val cache = DiskMediaCache(directory, 30_000)
        val editor = cache.begin("partial", 5000, CachedMediaMetadata(null, null, null, clock))!!
        editor.append(Buffer().writeUtf8("partial"), 0, 7)
        editor.abort()
        assertNull(cache.snapshot("partial", clock))
        assertNull(cache.begin("large", 40_000, CachedMediaMetadata(null, null, null, clock)))
        assertFalse(directory.listFiles()!!.any { it.extension == "part" })
    }

    @Test fun completedPreviewCanBeReusedForOfflineVideoRangeRequests() {
        val cache = DiskMediaCache(temporary.newFolder(), 1024 * 1024)
        val preview = url.replace("thumbnail", "preview")
        val client = client(cache) { response(it, "complete-video", 200, "video") }
        assertEquals("complete-video", read(client, preview))
        online = false
        val offline = client(cache) { throw IOException("Offline") }
        offline.newCall(Request.Builder().url(preview).header("Range", "bytes=4-").build()).execute().use {
            // HTTP permits a complete 200 response; Media3 skips to the requested offset locally.
            assertEquals(200, it.code)
            assertEquals("complete-video", it.body!!.string())
        }
    }

    @Test fun cacheLimitChoicesUseBinaryUnitsAndDefaultTo512MiB() {
        assertEquals(listOf(256L, 512L, 1024L, 2048L), MediaCacheLimit.entries.map { it.bytes / 1024 / 1024 })
        assertEquals(MediaCacheLimit.MIB_512, parseMediaCacheLimit(null))
        assertEquals(MediaCacheLimit.GIB_2, parseMediaCacheLimit("GIB_2"))
    }
    @Test fun invalidationPreventsAnOlderInFlightDownloadFromRepublishingTheEntry() {
        val cache = DiskMediaCache(temporary.newFolder(), 100_000)
        put(cache, "asset", "old")
        val pending = cache.begin("asset", 3, CachedMediaMetadata(null, "new", null, clock))!!
        cache.remove("asset")
        pending.append(Buffer().writeUtf8("new"), 0, 3)
        assertNull(cache.snapshot("asset", clock))
        assertNull(cache.begin("overflow", Long.MAX_VALUE, CachedMediaMetadata(null, null, null, clock)))
    }
}
