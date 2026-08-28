package io.github.yzard.momento.feature.media

data class CursorPagingState<Entry>(
    val entries: List<Entry>,
    val nextCursor: String?,
    val hasMore: Boolean,
    val loading: Boolean,
    val initialized: Boolean,
    val error: String?,
)

fun <Entry> emptyCursorPagingState(): CursorPagingState<Entry> = CursorPagingState(
    entries = emptyList(),
    nextCursor = null,
    hasMore = true,
    loading = false,
    initialized = false,
    error = null,
)

fun <Entry> beginCursorPage(
    state: CursorPagingState<Entry>,
    reset: Boolean,
): CursorPagingState<Entry>? {
    if (state.loading) return null
    if (!reset && (!state.hasMore || state.nextCursor == null)) return null
    if (!reset) return state.copy(loading = true, error = null)

    return emptyCursorPagingState<Entry>().copy(loading = true)
}

fun <Entry, Key> completeCursorPage(
    state: CursorPagingState<Entry>,
    page: List<Entry>,
    nextCursor: String?,
    hasMore: Boolean,
    key: (Entry) -> Key,
): CursorPagingState<Entry> {
    val knownKeys = state.entries.mapTo(mutableSetOf(), key)
    val uniquePage = page.filter { entry -> knownKeys.add(key(entry)) }
    return state.copy(
        entries = state.entries + uniquePage,
        nextCursor = nextCursor,
        hasMore = hasMore,
        loading = false,
        initialized = true,
        error = null,
    )
}

fun <Entry> failCursorPage(
    state: CursorPagingState<Entry>,
    message: String,
): CursorPagingState<Entry> = state.copy(
    loading = false,
    error = message,
)
