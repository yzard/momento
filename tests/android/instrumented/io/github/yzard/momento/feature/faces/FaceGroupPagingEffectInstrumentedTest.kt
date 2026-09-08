package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.feature.media.beginCursorPage
import io.github.yzard.momento.feature.media.completeCursorPage
import io.github.yzard.momento.feature.media.emptyCursorPagingState
import io.github.yzard.momento.feature.media.failCursorPage
import kotlinx.coroutines.CompletableDeferred
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class FaceGroupPagingEffectInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    @Test
    fun pendingPageSurvivesLoadingRecompositionAndStopsAtLastPage() {
        val response = CompletableDeferred<Unit>()
        var requests = 0
        var cancelled = false
        var pagingState by mutableStateOf(
            emptyCursorPagingState<FaceGroup>().copy(
                entries = listOf(FaceGroup(1, 2, 1)),
                nextCursor = "100",
                initialized = true,
            ),
        )
        composeRule.setContent {
            val gridState = rememberLazyGridState()
            FaceGroupPagingEffect(gridState, pagingState) {
                val loading = beginCursorPage(pagingState, false) ?: return@FaceGroupPagingEffect
                pagingState = loading
                requests++
                try {
                    response.await()
                    pagingState = completeCursorPage(
                        loading, listOf(FaceGroup(2, 3, 2)), null, false, FaceGroup::faceGroupId,
                    )
                } finally {
                    cancelled = !response.isCompleted
                }
            }
            LazyVerticalGrid(GridCells.Fixed(2), Modifier.fillMaxSize(), state = gridState) {
                items(pagingState.entries, key = { it.faceGroupId }) { Text("${it.mediaCount} media") }
            }
        }
        composeRule.waitUntil { requests == 1 }
        composeRule.waitForIdle()
        composeRule.runOnIdle {
            assertTrue(pagingState.loading)
            assertFalse(cancelled)
            response.complete(Unit)
        }
        composeRule.waitUntil { pagingState.entries.size == 2 }
        composeRule.runOnIdle {
            assertFalse(pagingState.loading)
            assertFalse(pagingState.hasMore)
            assertEquals(1, requests)
        }
    }

    @Test
    fun failedPageKeepsEntriesAndDoesNotAutomaticallyRetry() {
        val response = CompletableDeferred<Unit>()
        var requests = 0
        var pagingState by mutableStateOf(
            emptyCursorPagingState<FaceGroup>().copy(
                entries = listOf(FaceGroup(1, 2, 1)),
                nextCursor = "100",
                initialized = true,
            ),
        )
        composeRule.setContent {
            val gridState = rememberLazyGridState()
            FaceGroupPagingEffect(gridState, pagingState) {
                val loading = beginCursorPage(pagingState, false) ?: return@FaceGroupPagingEffect
                pagingState = loading
                requests++
                response.await()
                pagingState = failCursorPage(loading, "Connection failed")
            }
            LazyVerticalGrid(GridCells.Fixed(2), Modifier.fillMaxSize(), state = gridState) {
                items(pagingState.entries, key = { it.faceGroupId }) { Text("${it.mediaCount} media") }
            }
        }
        composeRule.waitUntil { requests == 1 }
        composeRule.waitForIdle()
        composeRule.runOnIdle { response.complete(Unit) }
        composeRule.waitUntil { pagingState.error != null }
        composeRule.waitForIdle()
        composeRule.runOnIdle {
            assertFalse(pagingState.loading)
            assertEquals(1, pagingState.entries.size)
            assertEquals("100", pagingState.nextCursor)
            assertEquals(1, requests)
        }
    }
}
