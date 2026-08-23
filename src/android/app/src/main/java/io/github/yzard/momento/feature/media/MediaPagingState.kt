package io.github.yzard.momento.feature.media

import io.github.yzard.momento.core.model.Media

data class MediaPagingState(
    val items: List<Media>,
    val nextCursor: String?,
    val hasMore: Boolean,
    val loading: Boolean,
    val error: String?,
)

fun appendMediaPage(state: MediaPagingState, page: List<Media>, nextCursor: String?, hasMore: Boolean): MediaPagingState {
    val knownIds = state.items.mapTo(mutableSetOf()) { it.id }
    val merged = state.items + page.filter { knownIds.add(it.id) }
    return state.copy(items = merged, nextCursor = nextCursor, hasMore = hasMore, loading = false, error = null)
}

fun mediaPageFailed(state: MediaPagingState, message: String): MediaPagingState = state.copy(loading = false, error = message)
