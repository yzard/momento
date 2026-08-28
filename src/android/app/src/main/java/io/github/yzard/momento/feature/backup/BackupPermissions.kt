package io.github.yzard.momento.feature.backup

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat

enum class BackupMediaAccess {
    FULL,
    PARTIAL,
    DENIED,
}

enum class BackupLocationMetadataAccess {
    PRESERVED,
    DENIED,
}

fun backupReadPermissions(sdkVersion: Int): Array<String> = when {
    sdkVersion >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE -> arrayOf(
        android.Manifest.permission.READ_MEDIA_IMAGES,
        android.Manifest.permission.READ_MEDIA_VIDEO,
        android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
    )
    sdkVersion >= Build.VERSION_CODES.TIRAMISU -> arrayOf(
        android.Manifest.permission.READ_MEDIA_IMAGES,
        android.Manifest.permission.READ_MEDIA_VIDEO,
    )
    else -> arrayOf(android.Manifest.permission.READ_EXTERNAL_STORAGE)
}

fun backupPermissions(sdkVersion: Int): Array<String> = buildList {
    addAll(backupReadPermissions(sdkVersion))
    if (sdkVersion >= Build.VERSION_CODES.Q) {
        add(android.Manifest.permission.ACCESS_MEDIA_LOCATION)
    }
}.toTypedArray()

fun backupMediaAccess(sdkVersion: Int, grantedPermissions: Set<String>): BackupMediaAccess {
    if (sdkVersion < Build.VERSION_CODES.TIRAMISU) {
        return if (android.Manifest.permission.READ_EXTERNAL_STORAGE in grantedPermissions) {
            BackupMediaAccess.FULL
        } else {
            BackupMediaAccess.DENIED
        }
    }

    val imagesGranted = android.Manifest.permission.READ_MEDIA_IMAGES in grantedPermissions
    val videosGranted = android.Manifest.permission.READ_MEDIA_VIDEO in grantedPermissions
    if (imagesGranted && videosGranted) return BackupMediaAccess.FULL
    if (imagesGranted || videosGranted) return BackupMediaAccess.PARTIAL
    if (
        sdkVersion >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE &&
        android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED in grantedPermissions
    ) {
        return BackupMediaAccess.PARTIAL
    }
    return BackupMediaAccess.DENIED
}

fun currentBackupMediaAccess(context: Context): BackupMediaAccess {
    val grantedPermissions = backupReadPermissions(Build.VERSION.SDK_INT)
        .filterTo(mutableSetOf()) { permission ->
            ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
        }
    return backupMediaAccess(Build.VERSION.SDK_INT, grantedPermissions)
}

fun backupLocationMetadataAccess(
    sdkVersion: Int,
    grantedPermissions: Set<String>,
): BackupLocationMetadataAccess {
    if (sdkVersion < Build.VERSION_CODES.Q) return BackupLocationMetadataAccess.PRESERVED
    return if (android.Manifest.permission.ACCESS_MEDIA_LOCATION in grantedPermissions) {
        BackupLocationMetadataAccess.PRESERVED
    } else {
        BackupLocationMetadataAccess.DENIED
    }
}

fun currentBackupLocationMetadataAccess(context: Context): BackupLocationMetadataAccess {
    val grantedPermissions = setOfNotNull(
        android.Manifest.permission.ACCESS_MEDIA_LOCATION.takeIf { permission ->
            ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
        },
    )
    return backupLocationMetadataAccess(Build.VERSION.SDK_INT, grantedPermissions)
}

fun backupCanReadOriginalMedia(
    mediaAccess: BackupMediaAccess,
    locationMetadataAccess: BackupLocationMetadataAccess,
): Boolean = mediaAccess != BackupMediaAccess.DENIED &&
    locationMetadataAccess == BackupLocationMetadataAccess.PRESERVED

fun currentBackupCanReadOriginalMedia(context: Context): Boolean = backupCanReadOriginalMedia(
    currentBackupMediaAccess(context),
    currentBackupLocationMetadataAccess(context),
)

fun backupMediaAccessLabel(access: BackupMediaAccess): String = when (access) {
    BackupMediaAccess.FULL -> "All photos and videos are available for backup"
    BackupMediaAccess.PARTIAL -> "Only selected photos or media types are available for backup"
    BackupMediaAccess.DENIED -> "Photo and video access is required before backup can run"
}

fun backupLocationMetadataAccessLabel(access: BackupLocationMetadataAccess): String = when (access) {
    BackupLocationMetadataAccess.PRESERVED -> "Photo location metadata will be preserved"
    BackupLocationMetadataAccess.DENIED -> "Photo location access is required for lossless backup"
}
