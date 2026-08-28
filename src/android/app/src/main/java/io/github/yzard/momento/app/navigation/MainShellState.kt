package io.github.yzard.momento.app.navigation

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.core.model.Place
import io.github.yzard.momento.feature.timeline.TimelinePeriod

sealed interface MainRoute {
    data class Collection(val destination: Destination) : MainRoute
    data class AlbumDetail(val albumId: Long) : MainRoute
    data class PlaceDetail(val place: Place) : MainRoute
    data class FaceDetail(val group: FaceGroup) : MainRoute
    data class Viewer(val media: List<Media>, val index: Int) : MainRoute
}

data class LibraryChange(val sequence: Int, val mediaId: Long)

class MainShellState : ViewModel() {
    var routeStack by mutableStateOf<List<MainRoute>>(listOf(MainRoute.Collection(Destination.TIMELINE)))
        private set
    var libraryChange by mutableStateOf<LibraryChange?>(null)
        private set
    var timelinePeriod by mutableStateOf(TimelinePeriod.DAY)
        private set
    var timelineSearchQuery by mutableStateOf("")
        private set
    private var libraryChangeSequence by mutableIntStateOf(0)

    val destination: Destination
        get() = (routeStack.first() as MainRoute.Collection).destination

    val viewer: MainRoute.Viewer?
        get() = routeStack.lastOrNull() as? MainRoute.Viewer

    val currentRoute: MainRoute
        get() = routeStack.last()

    val contentRoute: MainRoute
        get() = routeStack.last { route -> route !is MainRoute.Viewer }

    fun navigate(destination: Destination) {
        routeStack = listOf(MainRoute.Collection(destination))
    }

    fun openViewer(media: List<Media>, index: Int) {
        require(media.isNotEmpty()) { "Viewer media must not be empty" }
        require(index in media.indices) { "Viewer index must reference an item" }
        routeStack = routeStack + MainRoute.Viewer(media, index)
    }

    fun openAlbum(albumId: Long) {
        require(albumId > 0L) { "Album ID must be positive" }
        openDetail(MainRoute.AlbumDetail(albumId))
    }

    fun openPlace(place: Place) {
        openDetail(MainRoute.PlaceDetail(place))
    }

    fun openFace(group: FaceGroup) {
        openDetail(MainRoute.FaceDetail(group))
    }

    fun markMediaChanged(mediaId: Long) {
        require(mediaId > 0L) { "Changed media ID must be positive" }
        libraryChangeSequence += 1
        libraryChange = LibraryChange(libraryChangeSequence, mediaId)
    }

    fun updateViewerIndex(index: Int) {
        val currentViewer = viewer ?: return
        require(index in currentViewer.media.indices) { "Viewer index must reference an item" }
        routeStack = routeStack.dropLast(1) + currentViewer.copy(index = index)
    }

    fun closeViewer() {
        if (viewer == null) return
        routeStack = routeStack.dropLast(1)
    }

    fun closeDetail() {
        if (currentRoute is MainRoute.Collection || currentRoute is MainRoute.Viewer) return
        routeStack = listOf(MainRoute.Collection(destination))
    }

    fun selectTimelinePeriod(period: TimelinePeriod) {
        timelinePeriod = period
    }

    fun updateTimelineSearchQuery(query: String) {
        timelineSearchQuery = query
    }

    private fun openDetail(detail: MainRoute) {
        require(currentRoute is MainRoute.Collection) { "A detail route can only open from a collection" }
        routeStack = routeStack + detail
    }
}
