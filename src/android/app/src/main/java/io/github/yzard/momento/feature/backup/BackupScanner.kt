package io.github.yzard.momento.feature.backup

import android.content.ContentUris
import android.content.Context
import android.os.Build
import android.provider.MediaStore
import io.github.yzard.momento.core.database.BackupAssetDao
import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.model.BackupState
import java.util.UUID

class MediaStoreScanner(
    private val context: Context,
    private val assets: BackupAssetDao,
    private val backupGeneration: String?,
) {
    suspend fun scan(cameraOnly: Boolean) {
        val volumeNames = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.getExternalVolumeNames(context).sorted()
        } else {
            listOf("external")
        }
        volumeNames.forEach { volumeName -> scanVolume(volumeName, cameraOnly) }
    }

    private suspend fun scanVolume(volumeName: String, cameraOnly: Boolean) {
        val collection = MediaStore.Files.getContentUri(volumeName)
        val projection = buildList {
            add(MediaStore.Files.FileColumns._ID)
            add(MediaStore.Files.FileColumns.DISPLAY_NAME)
            add(MediaStore.Files.FileColumns.MIME_TYPE)
            add(MediaStore.Files.FileColumns.SIZE)
            add(MediaStore.Files.FileColumns.DATE_MODIFIED)
            add(MediaStore.Images.ImageColumns.BUCKET_DISPLAY_NAME)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                add(MediaStore.MediaColumns.RELATIVE_PATH)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                add(MediaStore.MediaColumns.GENERATION_MODIFIED)
            }
        }.toTypedArray()
        val selection = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (?, ?)"
        val arguments = arrayOf(
            MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
            MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString(),
        )
        context.contentResolver.query(collection, projection, selection, arguments, "${MediaStore.Files.FileColumns.DATE_MODIFIED} DESC")?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Files.FileColumns._ID)
            val displayNameColumn = cursor.getColumnIndexOrThrow(MediaStore.Files.FileColumns.DISPLAY_NAME)
            val mimeTypeColumn = cursor.getColumnIndexOrThrow(MediaStore.Files.FileColumns.MIME_TYPE)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Files.FileColumns.SIZE)
            val modifiedColumn = cursor.getColumnIndexOrThrow(MediaStore.Files.FileColumns.DATE_MODIFIED)
            val bucketColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.ImageColumns.BUCKET_DISPLAY_NAME)
            val relativePathColumn = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.RELATIVE_PATH)
            } else {
                -1
            }
            val generationModifiedColumn = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.GENERATION_MODIFIED)
            } else {
                -1
            }
            while (cursor.moveToNext()) {
                val mimeType = cursor.getString(mimeTypeColumn) ?: continue
                val bucketName = cursor.getString(bucketColumn)
                val relativePath = relativePathColumn.takeIf { it >= 0 }?.let(cursor::getString)
                val cameraMedia = isCameraMediaFolder(bucketName, relativePath)
                if (cameraOnly && !cameraMedia) continue
                val mediaStoreId = cursor.getLong(idColumn)
                val asset = discoveredAsset(
                    uri = ContentUris.withAppendedId(collection, mediaStoreId).toString(),
                    mediaStoreId = mediaStoreId,
                    volumeName = volumeName,
                    backupGeneration = backupGeneration,
                    displayName = cursor.getString(displayNameColumn) ?: "media",
                    mimeType = mimeType,
                    byteSize = cursor.getLong(sizeColumn),
                    modifiedAt = cursor.getLong(modifiedColumn),
                    generationModified = generationModifiedColumn
                        .takeIf { columnIndex -> columnIndex >= 0 }
                        ?.let(cursor::getLong)
                        ?: 0,
                    folder = if (cameraMedia) CAMERA_FOLDER else relativePath ?: bucketName.orEmpty(),
                )
                if (assets.insertDiscovered(asset) == -1L) {
                    assets.reconcileDiscovered(asset.uri, asset.volumeName, asset.clientAssetId, asset.operationId, asset.displayName, asset.mimeType, asset.byteSize, asset.modifiedAt, asset.generationModified, asset.folder)
                }
            }
        }
    }
}

internal fun isCameraMediaFolder(bucketName: String?, relativePath: String?): Boolean {
    val normalizedPath = relativePath
        ?.replace('\\', '/')
        ?.trim('/')
        ?.lowercase()
    if (normalizedPath != null) {
        val pathSegments = normalizedPath.split('/')
        if (pathSegments.any { it in CAMERA_DIRECTORY_EXCLUSIONS }) return false
        if (pathSegments.firstOrNull() == "dcim") return true
        return pathSegments.firstOrNull() == "pictures" && pathSegments.getOrNull(1) in KNOWN_CAMERA_BUCKETS
    }

    val normalizedBucket = bucketName?.trim()?.lowercase() ?: return false
    return normalizedBucket in KNOWN_CAMERA_BUCKETS || normalizedBucket.endsWith(" camera")
}

internal fun backupClientAssetId(
    mediaStoreId: Long,
    volumeName: String,
    backupGeneration: String?,
): String {
    val volumeToken = volumeName.map { character ->
        if (character.isLetterOrDigit()) character else '_'
    }.joinToString("")
    val generationToken = backupGeneration?.let { generation -> "${generation}_" }.orEmpty()
    val volumePrefix = if (volumeName == "external") "" else "${volumeToken}_"
    return "media_${generationToken}${volumePrefix}$mediaStoreId"
}

internal fun discoveredAsset(
    uri: String,
    mediaStoreId: Long,
    volumeName: String,
    backupGeneration: String?,
    displayName: String,
    mimeType: String,
    byteSize: Long,
    modifiedAt: Long,
    generationModified: Long,
    folder: String,
): BackupAssetEntity = BackupAssetEntity(
    uri,
    volumeName,
    backupClientAssetId(mediaStoreId, volumeName, backupGeneration),
    UUID.randomUUID().toString(),
    displayName,
    mimeType,
    byteSize,
    modifiedAt,
    generationModified,
    folder,
    BackupState.QUEUED,
    null,
    0,
    null,
    null,
    0,
    null,
)

private const val CAMERA_FOLDER = "Camera"
private val CAMERA_DIRECTORY_EXCLUSIONS = setOf(
    ".thumbnails",
    "screen recordings",
    "screen_recordings",
    "screenrecorder",
    "screenrecords",
    "screenshots",
)
private val KNOWN_CAMERA_BUCKETS = setOf("camera", "camera roll", "100andro", "100media", "dcim")

