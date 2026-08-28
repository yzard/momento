package io.github.yzard.momento.app.navigation

import io.github.yzard.momento.core.model.Capabilities

sealed interface CapabilityState {
    data object Loading : CapabilityState
    data class Available(val capabilities: Capabilities) : CapabilityState
    data class Failed(val message: String) : CapabilityState
}

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

fun Destination.isAvailable(capabilityState: CapabilityState): Boolean {
    val capabilities = (capabilityState as? CapabilityState.Available)?.capabilities
    return when (this) {
        Destination.SCREENSHOTS -> capabilities?.features?.screenshotDetection == true
        Destination.DOCUMENTS -> capabilities?.features?.documentDetection == true
        Destination.FACES -> capabilities?.features?.faceDetection == true
        Destination.DEDUPLICATE -> capabilities?.features?.deduplicate == true
        else -> true
    }
}

fun CapabilityState.backupAvailable(): Boolean =
    (this as? CapabilityState.Available)?.capabilities?.backup?.enabled == true
