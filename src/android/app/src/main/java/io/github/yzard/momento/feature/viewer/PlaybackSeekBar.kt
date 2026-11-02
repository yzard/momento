package io.github.yzard.momento.feature.viewer

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Slider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.unit.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlaybackSeekBar(
    positionMs: Float,
    durationMs: Long,
    dragging: Boolean,
    color: Color,
    onValueChange: (Float) -> Unit,
    onValueChangeFinished: () -> Unit,
    modifier: Modifier,
) {
    val geometry = playbackSliderGeometry(dragging)
    Slider(
        value = positionMs,
        onValueChange = onValueChange,
        onValueChangeFinished = onValueChangeFinished,
        valueRange = 0f..durationMs.coerceAtLeast(1L).toFloat(),
        enabled = durationMs > 0,
        modifier = modifier.height(48.dp),
        thumb = { Spacer(Modifier.size(geometry.trackSlotHeightDp.dp)) },
        track = { state ->
            Canvas(Modifier.fillMaxWidth().height(geometry.trackSlotHeightDp.dp)) {
                val progress = playbackProgressFraction(state.value, durationMs)
                val cursor = Offset(size.width * progress, center.y)
                drawLine(
                    color = color.copy(alpha = 0.32f),
                    start = Offset(0f, center.y),
                    end = Offset(size.width, center.y),
                    strokeWidth = geometry.trackStrokeDp.dp.toPx(),
                    cap = StrokeCap.Round,
                )
                drawLine(
                    color = color,
                    start = Offset(0f, center.y),
                    end = cursor,
                    strokeWidth = geometry.trackStrokeDp.dp.toPx(),
                    cap = StrokeCap.Round,
                )
                drawCircle(
                    color = color,
                    radius = geometry.thumbDiameterDp.dp.toPx() / 2f,
                    center = cursor,
                )
            }
        },
    )
}
