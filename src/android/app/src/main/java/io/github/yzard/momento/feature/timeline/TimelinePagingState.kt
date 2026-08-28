package io.github.yzard.momento.feature.timeline

import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.core.model.TimelineResponse
import io.github.yzard.momento.feature.media.PageState
import io.github.yzard.momento.feature.media.beginRefresh

enum class TimelineDirection(val wireValue: String) {
    OLDER("older"),
    NEWER("newer"),
}

data class TimelinePagingState(
    val page: PageState<List<TimelineGroup>>,
    val olderCursor: String?,
    val newerCursor: String?,
    val hasOlder: Boolean,
    val hasNewer: Boolean,
    val loadingDirection: TimelineDirection?,
    val failedDirection: TimelineDirection?,
    val message: String?,
) {
    fun cursor(direction: TimelineDirection): String? = when (direction) {
        TimelineDirection.OLDER -> olderCursor
        TimelineDirection.NEWER -> newerCursor
    }

    fun begin(reset: Boolean, direction: TimelineDirection): TimelinePagingState? {
        if (!reset && (loadingDirection != null || cursor(direction) == null)) return null
        if (reset) {
            return copy(
                page = page.beginRefresh(),
                olderCursor = null,
                newerCursor = null,
                hasOlder = false,
                hasNewer = false,
                loadingDirection = direction,
                failedDirection = null,
                message = null,
            )
        }
        return copy(
            loadingDirection = direction,
            failedDirection = null,
            message = null,
        )
    }

    fun complete(
        response: TimelineResponse,
        reset: Boolean,
        direction: TimelineDirection,
    ): TimelinePagingState {
        val currentGroups = (page as? PageState.Ready)?.content.orEmpty()
        val mergedGroups = when {
            reset -> response.groups
            direction == TimelineDirection.OLDER -> mergeTimelineGroups(currentGroups, response.groups)
            else -> mergeTimelineGroups(response.groups, currentGroups)
        }
        return copy(
            page = PageState.Ready(mergedGroups, refreshing = false),
            olderCursor = if (reset || direction == TimelineDirection.OLDER) response.nextCursor else olderCursor,
            newerCursor = if (reset || direction == TimelineDirection.NEWER) response.previousCursor else newerCursor,
            hasOlder = if (reset || direction == TimelineDirection.OLDER) response.hasOlder else hasOlder,
            hasNewer = if (reset || direction == TimelineDirection.NEWER) response.hasNewer else hasNewer,
            loadingDirection = null,
            failedDirection = null,
            message = null,
        )
    }

    fun fail(direction: TimelineDirection, message: String): TimelinePagingState {
        val failedPage = when (val currentPage = page) {
            PageState.Loading -> PageState.Failed(message)
            is PageState.Failed -> PageState.Failed(message)
            is PageState.Ready -> currentPage.copy(refreshing = false)
        }
        return copy(
            page = failedPage,
            loadingDirection = null,
            failedDirection = direction,
            message = message,
        )
    }

    companion object {
        fun initial(): TimelinePagingState = TimelinePagingState(
            page = PageState.Loading,
            olderCursor = null,
            newerCursor = null,
            hasOlder = false,
            hasNewer = false,
            loadingDirection = null,
            failedDirection = null,
            message = null,
        )
    }
}
