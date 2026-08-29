package io.github.yzard.momento.app.navigation

import io.github.yzard.momento.core.model.Capabilities

sealed interface CapabilityState {
    data object Loading : CapabilityState
    data class Available(val capabilities: Capabilities) : CapabilityState
    data class Failed(val message: String) : CapabilityState
}

enum class Destination(val label: String) {
    TIMELINE("Timeline"),
    PHOTOS("Photos"),
    VIDEOS("Videos"),
    SCREENSHOTS("Screenshots"),
    DOCUMENTS("Documents"),
    SETTINGS("Settings"),
    ALBUMS("Albums"),
    MAP("Map"),
    PLACES("Places"),
    FACES("Faces"),
    DEDUPLICATE("Deduplicate"),
    TRASH("Trash"),
    ADMIN_IMPORT("Import"),
    ADMIN_METADATA("Metadata"),
    ADMIN_AI("AI"),
    ADMIN_USERS("User Management"),
}

val timelineSubpageDestinations = listOf(
    Destination.PHOTOS,
    Destination.VIDEOS,
    Destination.SCREENSHOTS,
    Destination.DOCUMENTS,
)

val utilityDrawerDestinations = listOf(Destination.DEDUPLICATE)

val adminDrawerDestinations = listOf(
    Destination.ADMIN_IMPORT,
    Destination.ADMIN_METADATA,
    Destination.ADMIN_AI,
    Destination.ADMIN_USERS,
)

fun adminDrawerDestinationsForRole(role: String?): List<Destination> =
    if (role == "admin") adminDrawerDestinations else emptyList()

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

fun Destination.isAdminPage(): Boolean = this in adminDrawerDestinations

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
