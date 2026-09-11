package io.github.yzard.momento.core.cache

import java.io.IOException
import java.util.concurrent.atomic.AtomicLong
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody
import okio.Buffer
import okio.BufferedSource
import okio.ForwardingSource
import okio.buffer
import okio.source

internal class MediaCacheInterceptor(
    private val cache: DiskMediaCache,
    private val namespace: () -> String?,
    private val online: () -> Boolean,
    private val now: () -> Long,
) : Interceptor {
    private val mapGeneration = AtomicLong()

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val scope = namespace() ?: return chain.proceed(request)
        val asset = isCachedMediaRequest(request)
        val metadata = (request.method == "POST" && request.url.encodedPath in OFFLINE_READ_PATHS) ||
            (request.method == "GET" && request.url.encodedPath == "/api/v1/client/capabilities")
        if (!asset && !metadata) {
            val response = chain.proceed(request)
            if (request.method != "GET" && request.url.pathSegments.last() !in READ_OPERATIONS && response.isSuccessful) {
                mapGeneration.incrementAndGet()
                try { cache.removePrefix(MAP_KEY_PREFIX); cache.removePrefix("faces-") } catch (_: IOException) { /* Best-effort invalidation. */ }
            }
            return response
        }
        val map = request.method == "POST" && request.url.encodedPath == "/api/v1/map/clusters"
        val generation = mapGeneration.get()
        val body = Buffer().also { request.body?.writeTo(it) }
        val digest = Buffer().writeUtf8(scope).writeUtf8("\n${request.method}\n${request.url}\n")
            .apply { write(body, body.size) }.sha256().hex()
        val key = if (map) MAP_KEY_PREFIX + digest else if (request.url.encodedPath.startsWith("/api/v1/faces/")) "faces-" + digest else digest
        val cached = try { cache.snapshot(key, now()) } catch (_: IOException) { null }
        if (cached != null && (!online() || ((asset || map) && now() - cached.metadata.validatedAt in 0 until (if (map) MAP_FRESH_MILLIS else FRESH_MILLIS)))) {
            return cached.response(request)
        }
        // Cache complete representations, including when the video player asks for a range.
        // Playback still streams; incomplete/cancelled transfers are never published.
        val networkRequest = request.newBuilder().apply {
            if (asset && (cached != null || request.header("Range") == "bytes=0-")) {
                removeHeader("Range"); removeHeader("If-Range")
            }
            if (asset && cached != null) {
                cached.metadata.etag?.let { header("If-None-Match", it) }
                    ?: cached.metadata.lastModified?.let { header("If-Modified-Since", it) }
            }
        }.build()
        val response = try { chain.proceed(networkRequest) } catch (error: IOException) {
            if (cached != null && namespace() == scope) return cached.response(request)
            cached?.close()
            throw error
        }
        if (namespace() != scope || ((map || request.url.encodedPath.startsWith("/api/v1/faces/")) && mapGeneration.get() != generation)) { cached?.close(); return response }
        if (response.code == 304 && cached != null) {
            response.close()
            try { cache.revalidated(cached, now()) } catch (_: IOException) { /* Keep the usable snapshot. */ }
            return cached.response(request)
        }
        if (response.code in 500..599 && cached != null) {
            response.close()
            return cached.response(request)
        }
        cached?.close()
        if (response.code in listOf(401, 403, 404, 410)) {
            try { cache.remove(key) } catch (_: IOException) { /* No cached response is served. */ }
        }
        val responseBody = response.body
        if (response.code != 200 || responseBody == null) return response
        val editor = try {
            cache.begin(key, responseBody.contentLength(), CachedMediaMetadata(
                responseBody.contentType()?.toString(), response.header("ETag"), response.header("Last-Modified"), now(),
            ))
        } catch (_: IOException) { null } ?: return response
        val source = object : ForwardingSource(responseBody.source()) {
            private fun abortCache() { try { editor.abort() } catch (_: IOException) { /* Best-effort local cache. */ } }
            override fun read(sink: Buffer, byteCount: Long): Long {
                val read = try { super.read(sink, byteCount) } catch (error: IOException) { abortCache(); throw error }
                try {
                    if (namespace() != scope || ((map || request.url.encodedPath.startsWith("/api/v1/faces/")) && mapGeneration.get() != generation)) editor.abort()
                    else if (read == -1L) editor.complete()
                    else editor.append(sink, sink.size - read, read)
                } catch (_: IOException) { abortCache() }
                return read
            }
            override fun close() { try { super.close() } finally { abortCache() } }
        }.buffer()
        return response.newBuilder().body(object : ResponseBody() {
            override fun contentType() = responseBody.contentType()
            override fun contentLength() = responseBody.contentLength()
            override fun source(): BufferedSource = source
        }).build()
    }

    private fun CachedMediaSnapshot.response(request: Request): Response = Response.Builder()
        .request(request).protocol(Protocol.HTTP_1_1).code(200).message("Cached")
        .apply {
            metadata.contentType?.let { header("Content-Type", it) }
            metadata.etag?.let { header("ETag", it) }
            metadata.lastModified?.let { header("Last-Modified", it) }
            header("Content-Length", length.toString())
        }
        .body(object : ResponseBody() {
            private val source = input.source().buffer()
            override fun contentType() = metadata.contentType?.toMediaTypeOrNull()
            override fun contentLength() = length
            override fun source(): BufferedSource = source
        }).build()

    internal companion object {
        const val MAP_FRESH_MILLIS = 30_000L
        private val READ_OPERATIONS = setOf("get", "list", "status", "markers", "authenticate", "refresh")
        private const val MAP_KEY_PREFIX = "map-"
        const val FRESH_MILLIS = 5L * 60 * 1000
        private val MEDIA_PATH = Regex("/api/v1/(media/[^/]+/(thumbnail(/tiny)?|preview)|trash/[^/]+/thumbnail|faces/(groups|detections)/[^/]+/thumbnail|places/[^/]+/thumbnail)")
        fun isCachedMediaRequest(request: Request): Boolean = request.method == "GET" && MEDIA_PATH.matches(request.url.encodedPath)
        val OFFLINE_READ_PATHS = setOf(
            "/api/v1/user/get", "/api/v1/timeline/list", "/api/v1/album/list", "/api/v1/album/get",
            "/api/v1/places/list", "/api/v1/places/get", "/api/v1/faces/groups/list", "/api/v1/faces/groups/get",
            "/api/v1/duplicates/list", "/api/v1/trash/list", "/api/v1/map/clusters", "/api/v1/map/media",
        )
    }
}
