package io.github.yzard.momento.feature.viewer

import androidx.media3.common.C
import androidx.media3.common.Timeline
import androidx.media3.exoplayer.source.SinglePeriodTimeline
import org.junit.Assert.assertEquals
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import io.github.yzard.momento.core.cache.DiskMediaCache
import io.github.yzard.momento.core.cache.MediaCacheInterceptor
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import okhttp3.MediaType.Companion.toMediaType
import androidx.media3.exoplayer.ExoPlayer
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class ViewerVideoPlayerInstrumentedTest {
    @Test fun fragmentedPreviewWithoutSidxHasDurationProgressAndSeeks() = withFixture { fixture ->
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        verifyPlayback(DefaultDataSource.Factory(context), android.net.Uri.fromFile(fixture))
    }

    @Test fun cachedHttpPreviewUsesRangesForItsTailIndexAndSeeks() = withFixture { fixture ->
        val directory = File(fixture.parentFile, "http-cache-${System.nanoTime()}").apply { mkdirs() }
        try {
            val cache = DiskMediaCache(directory, 1024 * 1024)
            var online = true
            var requests = 0
            val client = OkHttpClient.Builder()
                .addInterceptor(MediaCacheInterceptor(cache, { "fixture" }, { online }, { 1000L }))
                .addInterceptor { chain ->
                    check(online)
                    requests++
                    Response.Builder().request(chain.request()).protocol(Protocol.HTTP_1_1)
                        .code(200).message("Fixture").header("ETag", "fixture-video")
                        .body(fixture.readBytes().toResponseBody("video/mp4".toMediaType())).build()
                }.build()
            val url = "http://127.0.0.1/api/v1/media/10915/preview"
            client.newCall(Request.Builder().url(url).build()).execute().use { it.body!!.bytes() }
            online = false
            verifyPlayback(OkHttpDataSource.Factory(client), android.net.Uri.parse(url))
            assertTrue("playback and seeking use the complete offline representation", requests == 1)
        } finally { directory.deleteRecursively() }
    }

    @Test fun metadataDurationFillsMissingTimeWithoutReplacingParsedDurationOrSeekability() {
        for (parsed in listOf(C.TIME_UNSET, 0L, 12_000_000L)) {
            for (seekable in listOf(false, true)) {
                val source = SinglePeriodTimeline(parsed, seekable, false, false, null, MediaItem.EMPTY)
                val timeline = MetadataDurationTimeline(source, 10_000_000)
                val expected = if (parsed <= 0) 10_000_000L else parsed
                assertEquals(expected, timeline.getWindow(0, Timeline.Window()).durationUs)
                assertEquals(expected, timeline.getPeriod(0, Timeline.Period()).durationUs)
                assertEquals(seekable, timeline.getWindow(0, Timeline.Window()).isSeekable)
            }
        }
        assertEquals(208681L, viewerDurationHintMs(208.681667))
        for (invalid in listOf(null, Double.NaN, Double.POSITIVE_INFINITY, -1.0, Double.MAX_VALUE)) {
            assertEquals(0L, viewerDurationHintMs(invalid))
        }
    }

    private fun withFixture(test: (File) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val fixture = File.createTempFile("fragmented-preview-", ".mp4", instrumentation.targetContext.cacheDir)
        try {
            instrumentation.context.assets.open("media/fragmented-preview.mp4").use { input -> fixture.outputStream().use { input.copyTo(it) } }
            test(fixture)
        } finally { fixture.delete() }
    }

    private fun verifyPlayback(dataSource: DataSource.Factory, uri: android.net.Uri) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        var player: ExoPlayer? = null
        fun onMain(action: () -> Unit) = instrumentation.runOnMainSync(action)
        fun awaitState(message: String, condition: (ExoPlayer) -> Boolean) {
            val deadline = System.currentTimeMillis() + 15000
            var satisfied = false
            while (!satisfied && System.currentTimeMillis() < deadline) {
                onMain { player!!.playerError?.let { throw AssertionError(message, it) }; satisfied = condition(player!!) }
                if (!satisfied) Thread.sleep(50)
            }
            assertTrue(message, satisfied)
        }
        try {
            onMain {
                player = createViewerVideoPlayer(context, dataSource, 10000).apply {
                    volume = 0f
                    setMediaItem(MediaItem.fromUri(uri))
                    prepare()
                }
            }
            awaitState("mfra index supplies a finite duration and seekable timeline") {
                it.playbackState == Player.STATE_READY && it.isCurrentMediaItemSeekable && it.duration in 9900..10500
            }
            onMain { player!!.play() }
            awaitState("playback position advances") { it.currentPosition > 700 }
            onMain { player!!.pause(); player!!.seekTo(6500) }
            awaitState("seeking lands near the requested timestamp") { it.playbackState == Player.STATE_READY && it.currentPosition in 6200..6800 }
        } finally {
            onMain { player?.release() }
        }
    }
}
