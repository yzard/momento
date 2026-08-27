package io.github.yzard.momento.app.navigation

import io.github.yzard.momento.core.model.Capabilities

enum class Destination(val label: String) {
    TIMELINE("Timeline"), PHOTOS("Photos"), VIDEOS("Videos"), SCREENSHOTS("Screenshots"), DOCUMENTS("Documents"), SETTINGS("Settings"), ALBUMS("Albums"), MAP("Map"), PLACES("Places"), FACES("Faces"), DEDUPLICATE("Deduplicate"), TRASH("Trash"), ADMIN("Admin")
}

val timelineSubpageDestinations = listOf(
    Destination.PHOTOS,
    Destination.VIDEOS,
    Destination.SCREENSHOTS,
    Destination.DOCUMENTS,
)

val utilityDrawerDestinations = listOf(Destination.DEDUPLICATE)

val webDrawerDestinations = listOf(
    Destination.TIMELINE,
    *timelineSubpageDestinations.toTypedArray(),
    Destination.ALBUMS,
    Destination.MAP,
    Destination.PLACES,
    Destination.FACES,
    *utilityDrawerDestinations.toTypedArray(),
    Destination.TRASH,
)

fun Destination.isTimelinePage(): Boolean = when (this) {
    Destination.TIMELINE,
    in timelineSubpageDestinations -> true
    else -> false
}

fun Destination.hasShellPageTitle(): Boolean = this != Destination.SETTINGS &&
    this != Destination.ALBUMS &&
    this != Destination.ADMIN

fun Destination.isAvailable(capabilities: Capabilities?): Boolean {
    if (capabilities == null) return true
    return when (this) {
        Destination.SCREENSHOTS -> capabilities.features.screenshotDetection
        Destination.DOCUMENTS -> capabilities.features.documentDetection
        Destination.FACES -> capabilities.features.faceDetection
        Destination.DEDUPLICATE -> capabilities.features.deduplicate
        else -> true
    }
}
