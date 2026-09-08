package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.feature.media.CursorPagingState
import io.github.yzard.momento.feature.media.shouldLoadMoreMedia
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch

@Composable
internal fun FaceGroupPagingEffect(
    gridState: LazyGridState,
    pagingState: CursorPagingState<FaceGroup>,
    loadMore: suspend () -> Unit,
) {
    val requestScope = rememberCoroutineScope()
    LaunchedEffect(gridState, pagingState.hasMore, pagingState.loading, pagingState.error) {
        snapshotFlow {
            val layout = gridState.layoutInfo
            pagingState.error == null && shouldLoadMoreMedia(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasMore = pagingState.hasMore,
                loading = pagingState.loading,
            )
        }.filter { it }.collect {
            // Loading restarts the observer, but must not cancel its in-flight request.
            requestScope.launch { loadMore() }
        }
    }
}
