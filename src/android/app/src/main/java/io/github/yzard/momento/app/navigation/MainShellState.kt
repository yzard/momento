package io.github.yzard.momento.app.navigation

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.timeline.TimelinePeriod

class MainShellState : ViewModel() {
    var destination by mutableStateOf(Destination.TIMELINE)
        private set
    var viewerMedia by mutableStateOf<List<Media>?>(null)
        private set
    var viewerIndex by mutableIntStateOf(0)
        private set
    var contentRevision by mutableIntStateOf(0)
        private set
    var timelinePeriod by mutableStateOf(TimelinePeriod.DAY)
        private set
    var timelineSearchQuery by mutableStateOf("")
        private set
    private var viewerChanged = false

    fun navigate(destination: Destination) {
        this.destination = destination
    }

    fun openViewer(media: List<Media>, index: Int) {
        require(media.isNotEmpty()) { "Viewer media must not be empty" }
        require(index in media.indices) { "Viewer index must reference an item" }
        viewerMedia = media
        viewerIndex = index
        viewerChanged = false
    }

    fun markViewerChanged() {
        viewerChanged = true
    }

    fun updateViewerIndex(index: Int) {
        val media = viewerMedia ?: return
        require(index in media.indices) { "Viewer index must reference an item" }
        viewerIndex = index
    }

    fun closeViewer() {
        viewerMedia = null
        if (!viewerChanged) return

        contentRevision += 1
        viewerChanged = false
    }

    fun selectTimelinePeriod(period: TimelinePeriod) {
        timelinePeriod = period
    }

    fun updateTimelineSearchQuery(query: String) {
        timelineSearchQuery = query
    }
}
