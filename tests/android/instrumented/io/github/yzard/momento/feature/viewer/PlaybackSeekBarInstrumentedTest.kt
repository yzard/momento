package io.github.yzard.momento.feature.viewer

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Density
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import kotlin.math.abs

class PlaybackSeekBarInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    @Test
    fun cursorAndTrackShareTheirRenderedCenterAcrossWidthsAndDragStates() {
        var width by mutableStateOf(280.dp)
        var dragging by mutableStateOf(false)
        var position by mutableFloatStateOf(50f)
        composeRule.setContent {
            CompositionLocalProvider(LocalDensity provides Density(1f)) {
                Box(Modifier.width(width).background(Color.Black).testTag("seek")) {
                    PlaybackSeekBar(
                        positionMs = position,
                        durationMs = 100,
                        dragging = dragging,
                        color = Color.White,
                        onValueChange = {},
                        onValueChangeFinished = {},
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
        for (viewport in listOf(280.dp, 640.dp)) {
            for (isDragging in listOf(false, true)) {
                for (progress in listOf(0f, 50f, 100f)) {
                    composeRule.runOnIdle {
                        width = viewport
                        dragging = isDragging
                        position = progress
                    }
                    composeRule.mainClock.advanceTimeByFrame()
                    composeRule.waitForIdle()
                    val pixels = composeRule.onNodeWithTag("seek").captureToImage().toPixelMap()
                    assertEquals(viewport.value.toInt(), pixels.width)
                    fun coloredRows(x: Int) = (0 until pixels.height).filter { pixels[x, it].red > 0.15f }
                    val cursorX = (0 until pixels.width).maxBy { coloredRows(it).size }
                    val cursorRows = coloredRows(cursorX)
                    val trackRows = coloredRows(pixels.width / 4)
                    assertTrue("cursor is visible", cursorRows.isNotEmpty())
                    assertTrue("track is visible", trackRows.isNotEmpty())
                    assertTrue("cursor is thicker than track", cursorRows.size > trackRows.size)
                    val expectedCursorX = 7f + (pixels.width - 14f) * progress / 100f
                    assertTrue(
                        "cursor follows progress: width=${pixels.width}, progress=$progress, dragging=$isDragging, x=$cursorX, expected=$expectedCursorX",
                        abs(cursorX - expectedCursorX) <= 7f,
                    )
                    val cursorCenter = (cursorRows.first() + cursorRows.last()) / 2f
                    val trackCenter = (trackRows.first() + trackRows.last()) / 2f
                    assertTrue("cursor and track align", abs(cursorCenter - trackCenter) <= 1f)
                }
            }
        }
    }

    @Test
    fun draggingPreviewsPositionAndCommitsOnlyOnRelease() {
        var position by mutableFloatStateOf(0f)
        var commits = 0
        composeRule.setContent {
            PlaybackSeekBar(
                positionMs = position,
                durationMs = 100,
                dragging = true,
                color = Color.White,
                onValueChange = { position = it },
                onValueChangeFinished = { commits += 1 },
                modifier = Modifier.width(280.dp).testTag("seek"),
            )
        }
        composeRule.onNodeWithTag("seek").performTouchInput {
            down(Offset(width * 0.25f, height / 2f))
            moveTo(Offset(width * 0.75f, height / 2f))
        }
        composeRule.runOnIdle { assertEquals(0, commits) }
        composeRule.onNodeWithTag("seek").performTouchInput { up() }
        composeRule.runOnIdle {
            assertEquals(1, commits)
            assertTrue(position > 65f)
        }
    }
}
