package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createComposeRule
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.core.model.TimelineResponse
import io.github.yzard.momento.feature.media.PageState
import kotlinx.coroutines.CompletableDeferred
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Rule
import org.junit.Test

class TimelinePagingEffectInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    private fun group(id: Long) = TimelineGroup("2026-10-$id", listOf(
        Media(id, "$id.jpg", "$id.jpg", "image", createdAt = "2026-10-01"),
    ))

    @Test fun selectedPeriodLoadsBothNeighborsAndContinuesWithoutAnotherGesture() {
        val release = CompletableDeferred<Unit>()
        val requests = mutableListOf<TimelineDirection>()
        var cancelled = false
        var state by mutableStateOf(TimelinePagingState.initial().complete(
            TimelineResponse(listOf(group(3)), "older-1", "newer-1", true, true),
            true, TimelineDirection.OLDER,
        ))
        composeRule.setContent {
            val grid = rememberLazyGridState()
            TimelinePagingEffect(grid, state) { direction ->
                val loading = state.begin(false, direction) ?: return@TimelinePagingEffect
                state = loading
                requests += direction
                if (direction == TimelineDirection.NEWER) {
                    try { release.await() } finally { cancelled = !release.isCompleted }
                }
                val response = when (requests.size) {
                    1 -> TimelineResponse(listOf(group(4)), "unused", null, true, false)
                    2 -> TimelineResponse(listOf(group(2)), "older-2", null, true, false)
                    else -> TimelineResponse(listOf(group(1)), null, null, false, false)
                }
                state = loading.complete(response, false, direction)
            }
            val media = (state.page as PageState.Ready).content.flatMap { it.media }
            LazyVerticalGrid(GridCells.Fixed(2), Modifier.height(300.dp), state = grid) {
                items(media, key = { it.id }) { Text("${it.id}", Modifier.height(40.dp)) }
            }
        }
        composeRule.waitUntil { requests.size == 1 }
        composeRule.waitForIdle()
        composeRule.runOnIdle {
            assertFalse(cancelled)
            assertEquals(TimelineDirection.NEWER, state.loadingDirection)
            release.complete(Unit)
        }
        composeRule.waitUntil { requests.size == 3 && state.loadingDirection == null }
        composeRule.runOnIdle {
            assertEquals(listOf(TimelineDirection.NEWER, TimelineDirection.OLDER, TimelineDirection.OLDER), requests)
            assertEquals(listOf(4L, 3L, 2L, 1L), (state.page as PageState.Ready).content.flatMap { it.media }.map { it.id })
            assertFalse(state.hasOlder)
            assertFalse(state.hasNewer)
            assertFalse(cancelled)
        }
    }

    @Test fun failedNeighborDoesNotLoopOrDiscardTheSelectedPeriod() {
        var requests = 0
        var state by mutableStateOf(TimelinePagingState.initial().complete(
            TimelineResponse(listOf(group(3)), "older", null, true, false), true, TimelineDirection.OLDER,
        ))
        composeRule.setContent {
            val grid = rememberLazyGridState()
            TimelinePagingEffect(grid, state) { direction ->
                val loading = state.begin(false, direction) ?: return@TimelinePagingEffect
                state = loading
                requests++
                state = loading.fail(direction, "Offline")
            }
            LazyVerticalGrid(GridCells.Fixed(2), Modifier.height(300.dp), state = grid) {
                items((state.page as PageState.Ready).content.flatMap { it.media }, key = { it.id }) {
                    Text("${it.id}", Modifier.height(40.dp))
                }
            }
        }
        composeRule.waitUntil { state.message == "Offline" }
        composeRule.waitForIdle()
        composeRule.runOnIdle {
            assertEquals(1, requests)
            assertEquals(listOf(group(3)), (state.page as PageState.Ready).content)
        }
    }
}
