package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import io.github.yzard.momento.feature.media.PageState
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch

@Composable
internal fun TimelinePagingEffect(
    gridState: LazyGridState,
    pagingState: TimelinePagingState,
    load: suspend (TimelineDirection) -> Unit,
) {
    val requestScope = rememberCoroutineScope()
    LaunchedEffect(gridState, pagingState) {
        snapshotFlow {
            val ready = pagingState.page as? PageState.Ready
            val layout = gridState.layoutInfo
            val mediaCount = ready?.content?.sumOf { it.media.size } ?: 0
            // Wait for the grid to apply a new page and preserve its visible media key.
            if (ready == null || ready.refreshing || pagingState.loadingDirection != null ||
                pagingState.message != null || layout.totalItemsCount != mediaCount
            ) {
                null
            } else if (shouldPrependTimeline(
                layout.visibleItemsInfo.firstOrNull()?.index ?: -1,
                layout.visibleItemsInfo.size,
                pagingState.hasNewer,
                false,
            )) {
                TimelineDirection.NEWER
            } else if (shouldAppendTimeline(
                layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                layout.totalItemsCount,
                layout.visibleItemsInfo.size,
                pagingState.hasOlder,
                false,
            )) {
                TimelineDirection.OLDER
            } else {
                null
            }
        }.filterNotNull().collect { direction ->
            // State changes restart observation without cancelling the request it started.
            requestScope.launch { load(direction) }
        }
    }
}
