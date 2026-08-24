package io.github.yzard.momento.app.navigation

enum class Destination(val label: String) {
    TIMELINE("Timeline"), PHOTOS("Photos"), VIDEOS("Videos"), SCREENSHOTS("Screenshots"), DOCUMENTS("Documents"), COLLECTIONS("Collections"), CREATE("Create"), SEARCH("Search"), SETTINGS("Settings"), ALBUMS("Albums"), MAP("Map"), PLACES("Places"), FACES("Faces"), DEDUPLICATE("Deduplicate"), TRASH("Trash"), ADMIN("Admin"), VIEWER("Viewer")
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

fun Destination.hasFloatingTitle(): Boolean = this != Destination.SETTINGS
