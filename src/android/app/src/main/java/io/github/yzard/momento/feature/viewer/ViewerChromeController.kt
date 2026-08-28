package io.github.yzard.momento.feature.viewer

enum class ViewerSheet { ALBUMS, INFORMATION }

data class ViewerChromeState(
    val visible: Boolean,
    val interactionRevision: Int,
    val interactionActive: Boolean,
    val sheet: ViewerSheet?,
) {
    fun recordInteraction(): ViewerChromeState = copy(
        visible = true,
        interactionRevision = interactionRevision + 1,
    )

    fun changeInteraction(active: Boolean): ViewerChromeState = recordInteraction().copy(
        interactionActive = active,
    )

    fun toggle(): ViewerChromeState = copy(
        visible = !visible,
        interactionRevision = interactionRevision + 1,
    )

    fun openSheet(sheet: ViewerSheet): ViewerChromeState = recordInteraction().copy(sheet = sheet)

    fun closeSheet(): ViewerChromeState = recordInteraction().copy(sheet = null)

    fun hideAfterInactivity(): ViewerChromeState {
        if (!visible || interactionActive || sheet != null) return this
        return copy(visible = false)
    }

    companion object {
        fun initial(): ViewerChromeState = ViewerChromeState(
            visible = true,
            interactionRevision = 0,
            interactionActive = false,
            sheet = null,
        )
    }
}

fun ViewerChromeState.restorationValues(): List<String> = listOf(
    visible.toString(),
    interactionRevision.toString(),
    sheet?.name.orEmpty(),
)

fun restoreViewerChromeState(values: List<String>): ViewerChromeState? {
    if (values.size != 3) return null
    val visible = values[0].toBooleanStrictOrNull() ?: return null
    val interactionRevision = values[1].toIntOrNull()?.takeIf { it >= 0 } ?: return null
    val sheet = values[2].takeIf(String::isNotEmpty)?.let { serializedSheet ->
        ViewerSheet.entries.firstOrNull { it.name == serializedSheet } ?: return null
    }
    return ViewerChromeState(
        visible = visible,
        interactionRevision = interactionRevision,
        interactionActive = false,
        sheet = sheet,
    )
}

data class ViewerNavigationState(
    val currentIndex: Int,
    val itemCount: Int,
) {
    init {
        require(itemCount >= 0) { "Viewer item count cannot be negative" }
    }

    fun select(index: Int): ViewerNavigationState = copy(
        currentIndex = viewerIndex(index, 0, itemCount),
    )

    fun removeCurrent(): ViewerNavigationState {
        val remainingCount = (itemCount - 1).coerceAtLeast(0)
        return ViewerNavigationState(
            currentIndex = currentIndex.coerceAtMost((remainingCount - 1).coerceAtLeast(0)),
            itemCount = remainingCount,
        )
    }
}

data class ViewerSeekState(
    val positionMs: Long,
    val durationMs: Long,
    val previewPositionMs: Float?,
) {
    val dragging: Boolean get() = previewPositionMs != null

    val displayedPositionMs: Float
        get() = (previewPositionMs ?: positionMs.toFloat())
            .coerceIn(0f, durationMs.coerceAtLeast(1L).toFloat())

    fun synchronize(positionMs: Long, durationMs: Long): ViewerSeekState = copy(
        positionMs = if (dragging) this.positionMs else positionMs.coerceAtLeast(0L),
        durationMs = durationMs.coerceAtLeast(0L),
    )

    fun dragTo(positionMs: Float): ViewerSeekState = copy(
        previewPositionMs = positionMs.coerceIn(0f, durationMs.coerceAtLeast(1L).toFloat()),
    )

    fun cancelDrag(): ViewerSeekState = copy(previewPositionMs = null)

    fun commitDrag(): Pair<ViewerSeekState, Long>? {
        val preview = previewPositionMs ?: return null
        val committedPosition = boundedPlaybackPosition(preview, durationMs)
        return copy(positionMs = committedPosition, previewPositionMs = null) to committedPosition
    }

    companion object {
        fun initial(): ViewerSeekState = ViewerSeekState(
            positionMs = 0L,
            durationMs = 0L,
            previewPositionMs = null,
        )
    }
}
