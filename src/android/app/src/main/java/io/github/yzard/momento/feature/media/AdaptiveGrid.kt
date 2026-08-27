package io.github.yzard.momento.feature.media

enum class MomentoWindowClass {
    COMPACT,
    MEDIUM,
    EXPANDED,
}

fun momentoWindowClass(widthDp: Int): MomentoWindowClass = when {
    widthDp < 600 -> MomentoWindowClass.COMPACT
    widthDp < 840 -> MomentoWindowClass.MEDIUM
    else -> MomentoWindowClass.EXPANDED
}

fun adaptiveGridColumns(widthDp: Int): Int = when {
    widthDp < 360 -> 2
    widthDp < 480 -> 3
    widthDp < 600 -> 4
    widthDp < 840 -> 5
    widthDp < 1200 -> 6
    else -> 8
}

fun adaptiveContentPadding(widthDp: Int): Int = when (momentoWindowClass(widthDp)) {
    MomentoWindowClass.COMPACT -> 12
    MomentoWindowClass.MEDIUM -> 20
    MomentoWindowClass.EXPANDED -> 28
}

fun shouldLoadMoreMedia(
    lastVisibleItemIndex: Int,
    totalItemsCount: Int,
    hasMore: Boolean,
    loading: Boolean,
): Boolean = hasMore && !loading && totalItemsCount > 0 && lastVisibleItemIndex >= totalItemsCount - 3
