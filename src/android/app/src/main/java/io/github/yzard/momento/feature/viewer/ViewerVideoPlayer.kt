package io.github.yzard.momento.feature.viewer

import android.content.Context
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Timeline
import androidx.media3.datasource.DataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.ForwardingTimeline
import androidx.media3.exoplayer.source.MediaSource
import androidx.media3.exoplayer.source.WrappingMediaSource
import androidx.media3.extractor.DefaultExtractorsFactory
import androidx.media3.extractor.mp4.FragmentedMp4Extractor

/** Existing previews have an mfra seek index but may omit the container duration. */
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
internal fun createViewerVideoPlayer(context: Context, dataSource: DataSource.Factory, durationMs: Long): ExoPlayer {
    val extractors = DefaultExtractorsFactory()
        .setFragmentedMp4ExtractorFlags(FragmentedMp4Extractor.FLAG_READ_MFRA_FOR_SEEK_MAP)
    val delegate = DefaultMediaSourceFactory(context, extractors).setDataSourceFactory(dataSource)
    val factory = object : MediaSource.Factory by delegate {
        override fun createMediaSource(mediaItem: MediaItem): MediaSource {
            val source = delegate.createMediaSource(mediaItem)
            return if (durationMs > 0) MetadataDurationMediaSource(source, durationMs * 1000) else source
        }
    }
    return ExoPlayer.Builder(context).setMediaSourceFactory(factory).build()
}

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
private class MetadataDurationMediaSource(source: MediaSource, private val durationUs: Long) : WrappingMediaSource(source) {
    override fun onChildSourceInfoRefreshed(newTimeline: Timeline) {
        refreshSourceInfo(MetadataDurationTimeline(newTimeline, durationUs))
    }
}

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
internal class MetadataDurationTimeline(timeline: Timeline, private val knownDurationUs: Long) : ForwardingTimeline(timeline) {
    override fun getWindow(windowIndex: Int, window: Timeline.Window, defaultPositionProjectionUs: Long): Timeline.Window =
        super.getWindow(windowIndex, window, defaultPositionProjectionUs).apply {
            if (durationUs == C.TIME_UNSET || durationUs <= 0) durationUs = knownDurationUs
        }

    override fun getPeriod(periodIndex: Int, period: Timeline.Period, setIds: Boolean): Timeline.Period =
        super.getPeriod(periodIndex, period, setIds).apply {
            if (durationUs == C.TIME_UNSET || durationUs <= 0) durationUs = knownDurationUs
        }
}

internal fun viewerDurationHintMs(seconds: Double?): Long =
    seconds?.takeIf { it.isFinite() && it > 0 && it <= Long.MAX_VALUE / 1_000_000.0 }
        ?.let { (it * 1000).toLong() } ?: 0L
