package io.github.yzard.momento.feature.backup

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ContentUris
import android.content.Context
import android.database.Cursor
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.Uri
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.provider.MediaStore
import android.system.Os
import android.system.OsConstants
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.await
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.database.BackupAssetDao
import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupCapabilities
import io.github.yzard.momento.core.model.BackupUploadCreateRequest
import io.github.yzard.momento.core.model.BackupUploadResponse
import io.github.yzard.momento.core.network.NetworkClient
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody.Companion.toRequestBody
import retrofit2.HttpException
import java.io.FileNotFoundException
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.time.Instant
import java.util.UUID
import java.util.concurrent.TimeUnit

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
)

internal enum class BackupProgress { COMPLETED, WAITING_FOR_SERVER }

internal fun backupCanRun(capabilities: BackupCapabilities): Boolean =
    capabilities.enabled && capabilities.protocolVersion == LOSSLESS_BACKUP_PROTOCOL_VERSION

private class TerminalBackupException(message: String) : Exception(message)

internal fun serverProgress(response: BackupUploadResponse): BackupProgress = when (response.status) {
    "completed", "failed", "cancelled" -> BackupProgress.COMPLETED
    else -> BackupProgress.WAITING_FOR_SERVER
}

class BackupWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val database = BackupDatabase.create(applicationContext)
        return try {
            val assets = database.backupAssetDao()
            val settingsStore = SettingsStore(applicationContext)
            val tokenStore = EncryptedTokenStore(applicationContext)
            val repository = MomentoRepository(settingsStore, tokenStore, NetworkClient(tokenStore))
            val settings = settingsStore.settings.first()
            if (!tokenStore.isAuthenticated.value || settings.origin == null) return Result.failure()
            val mediaAccess = currentBackupMediaAccess(applicationContext)
            val locationMetadataAccess = currentBackupLocationMetadataAccess(applicationContext)
            if (!backupCanReadOriginalMedia(mediaAccess, locationMetadataAccess)) return Result.success()
            if (!isBackupNetworkAllowed(applicationContext, settings.mobileDataEnabled)) return Result.retry()

            setForeground(progress("Preparing backup"))
            val deviceId = settingsStore.deviceId()
            repository.registerBackupDevice(deviceId, "${Build.MANUFACTURER} ${Build.MODEL}".trim())
            MediaStoreScanner(
                applicationContext,
                assets,
                settingsStore.backupGeneration(),
            ).scan(settings.cameraOnly)
            val backupCapabilities = repository.capabilities(settings.origin).backup
            if (!backupCanRun(backupCapabilities)) return Result.success()
            val chunkSize = backupCapabilities.maxChunkBytes.coerceAtMost(1024L * 1024L).toInt()
            var waitingForServer = false
            for (asset in assets.pending(settings.cameraOnly)) {
                val backupProgress = transfer(
                    asset,
                    assets,
                    repository,
                    deviceId,
                    chunkSize,
                    locationMetadataAccess,
                )
                if (backupProgress == BackupProgress.COMPLETED) {
                    deleteBackupSnapshot(applicationContext, asset.operationId)
                } else {
                    waitingForServer = true
                }
            }
            if (waitingForServer) Result.retry() else Result.success()
        } catch (error: IOException) {
            Result.retry()
        } catch (error: HttpException) {
            if (isRetryable(error.code())) Result.retry() else Result.failure()
        } finally {
            database.close()
        }
    }

    private suspend fun transfer(
        asset: BackupAssetEntity,
        assets: BackupAssetDao,
        repository: MomentoRepository,
        deviceId: String,
        chunkSize: Int,
        locationMetadataAccess: BackupLocationMetadataAccess,
    ): BackupProgress {
        var activeUploadId = asset.uploadId
        var durableUploadedBytes = asset.uploadedBytes
        try {
            if (asset.state == BackupState.CANCELLING) {
                return cancelBackupAsset(asset, assets, repository)
            }
            val snapshot = prepareBackupSnapshot(applicationContext, asset, locationMetadataAccess)
            val createRequest = BackupUploadCreateRequest(
                protocolVersion = LOSSLESS_BACKUP_PROTOCOL_VERSION,
                deviceId = deviceId,
                clientAssetId = asset.clientAssetId,
                operationId = asset.operationId,
                originalFilename = asset.displayName,
                mimeType = asset.mimeType,
                byteSize = snapshot.byteSize,
                contentHash = snapshot.contentHash,
                sourceModifiedAt = Instant.ofEpochSecond(asset.modifiedAt).toString(),
                metadata = snapshot.metadata,
            )
            val existing = asset.uploadId?.let { repository.backupUploadStatus(it) }
            if (existing?.contentHash != null && existing.contentHash != snapshot.contentHash) {
                throw TerminalBackupException("Server upload does not match this original snapshot")
            }
            val upload = when {
                existing == null -> repository.createBackupUpload(createRequest)
                existing.contentHash == null -> repository.createBackupUpload(createRequest)
                else -> existing
            }
            if (existing != null) {
                val progress = recordServerStatus(asset, upload, assets)
                if (asset.state == BackupState.SERVER_PROCESSING || progress == BackupProgress.COMPLETED) return progress
                if (asset.state == BackupState.COMPLETING && upload.status != "uploading") return progress
            }

            activeUploadId = upload.uploadId
            durableUploadedBytes = upload.uploadedSize
            if (upload.status != "uploading") return recordServerStatus(asset, upload, assets)

            var offset = upload.uploadedSize
            assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, upload.uploadId, null, null)
            val mediaStream = try {
                snapshot.file.inputStream()
            } catch (error: FileNotFoundException) {
                throw TerminalBackupException(error.message ?: "Media asset is no longer available")
            }
            mediaStream.use { stream ->
                skipFully(stream, offset)
                val buffer = ByteArray(chunkSize)
                while (offset < snapshot.byteSize) {
                    val read = stream.read(buffer)
                    if (read <= 0) break
                    val end = offset + read - 1
                    val chunkHash = sha256Hex(buffer, read)
                    val response = repository.uploadBackupChunk(
                        upload.uploadId,
                        "bytes $offset-$end/${snapshot.byteSize}",
                        chunkHash,
                        buffer.copyOf(read).toRequestBody(asset.mimeType.toMediaTypeOrNull()),
                    )
                    offset = response.uploadedSize
                    durableUploadedBytes = offset
                    assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, response.uploadId, response.mediaId, response.error)
                    setForeground(progress("Uploading ${asset.displayName}"))
                }
            }
            if (offset != snapshot.byteSize) throw TerminalBackupException("Upload ended before source file")
            assets.updateTransfer(asset.uri, BackupState.COMPLETING, offset, upload.uploadId, null, null)
            return recordServerStatus(asset, repository.completeBackupUpload(upload.uploadId), assets)
        } catch (error: HttpException) {
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, "HTTP ${error.code()}")
            if (isRetryable(error.code())) throw error
            return BackupProgress.COMPLETED
        } catch (error: IOException) {
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, error.message)
            throw error
        } catch (error: TerminalBackupException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                ),
                assets,
                repository,
            )
        } catch (error: SecurityException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                ),
                assets,
                repository,
            )
        } catch (error: IllegalArgumentException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                ),
                assets,
                repository,
            )
        }
    }

    private suspend fun recordServerStatus(asset: BackupAssetEntity, response: BackupUploadResponse, assets: BackupAssetDao): BackupProgress = when (response.status) {
        "completed" -> {
            assets.updateTransfer(asset.uri, BackupState.COMPLETED, response.uploadedSize, response.uploadId, response.mediaId, response.error)
            BackupProgress.COMPLETED
        }
        "failed" -> {
            assets.updateTransfer(asset.uri, BackupState.TERMINAL_FAILED, response.uploadedSize, response.uploadId, response.mediaId, response.error ?: "Server upload ${response.status}")
            BackupProgress.COMPLETED
        }
        "cancelled" -> {
            assets.updateTransfer(asset.uri, BackupState.CANCELLED, response.uploadedSize, response.uploadId, response.mediaId, response.error)
            BackupProgress.COMPLETED
        }
        else -> {
            assets.updateTransfer(asset.uri, BackupState.SERVER_PROCESSING, response.uploadedSize, response.uploadId, response.mediaId, response.error)
            BackupProgress.WAITING_FOR_SERVER
        }
    }

    private fun skipFully(stream: java.io.InputStream, offset: Long) {
        var remaining = offset
        while (remaining > 0) {
            val skipped = stream.skip(remaining)
            if (skipped > 0) {
                remaining -= skipped
                continue
            }
            if (stream.read() == -1) throw TerminalBackupException("Media asset ended before the server offset")
            remaining -= 1
        }
    }

    private fun progress(text: String): ForegroundInfo {
        val notification = NotificationCompat.Builder(applicationContext, BACKUP_CHANNEL).setSmallIcon(android.R.drawable.stat_sys_upload).setContentTitle("Momento backup").setContentText(text).setOngoing(true).build()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return ForegroundInfo(42, notification)
        return ForegroundInfo(42, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    }
}

internal suspend fun cancelBackupAsset(
    asset: BackupAssetEntity,
    assets: BackupAssetDao,
    repository: MomentoRepository,
): BackupProgress {
    val uploadId = asset.uploadId
    if (uploadId == null) {
        assets.updateTransfer(asset.uri, BackupState.CANCELLED, 0, null, null, null)
        return BackupProgress.COMPLETED
    }
    return try {
        recordCancellationStatus(asset, repository.cancelBackupUpload(uploadId), assets)
    } catch (error: IOException) {
        assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, error.message)
        BackupProgress.WAITING_FOR_SERVER
    } catch (error: HttpException) {
        if (!isCancellationRetryable(error.code())) {
            assets.updateTransfer(asset.uri, BackupState.TERMINAL_FAILED, asset.uploadedBytes, uploadId, asset.mediaId, "HTTP ${error.code()} while cancelling")
            return BackupProgress.COMPLETED
        }
        val current = try {
            repository.backupUploadStatus(uploadId)
        } catch (_: IOException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, error.message)
            return BackupProgress.WAITING_FOR_SERVER
        } catch (statusError: HttpException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, "HTTP ${statusError.code()} while checking cancellation")
            return BackupProgress.WAITING_FOR_SERVER
        }
        recordCancellationStatus(asset, current, assets)
    }
}

private suspend fun recordCancellationStatus(
    asset: BackupAssetEntity,
    response: BackupUploadResponse,
    assets: BackupAssetDao,
): BackupProgress {
    val state = cancellationState(response.status)
    val error = response.error ?: if (state == BackupState.TERMINAL_FAILED) "Server upload failed" else null
    assets.updateTransfer(asset.uri, state, response.uploadedSize, response.uploadId, response.mediaId, error)
    if (state == BackupState.CANCELLING || state == BackupState.SERVER_PROCESSING) {
        return BackupProgress.WAITING_FOR_SERVER
    }
    return BackupProgress.COMPLETED
}

internal fun cancellationState(status: String): BackupState = when (status) {
    "cancelled" -> BackupState.CANCELLED
    "completed" -> BackupState.COMPLETED
    "failed" -> BackupState.TERMINAL_FAILED
    "processing" -> BackupState.SERVER_PROCESSING
    else -> BackupState.CANCELLING
}

internal fun isCancellationRetryable(statusCode: Int): Boolean =
    statusCode == 409 || isRetryable(statusCode)

class BackupCancellationWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val database = BackupDatabase.create(applicationContext)
        return try {
            val settingsStore = SettingsStore(applicationContext)
            val tokenStore = EncryptedTokenStore(applicationContext)
            if (!tokenStore.isAuthenticated.value || settingsStore.settings.first().origin == null) return Result.failure()
            val repository = MomentoRepository(settingsStore, tokenStore, NetworkClient(tokenStore))
            var waiting = false
            for (asset in database.backupAssetDao().cancellationPending()) {
                if (cancelBackupAsset(asset, database.backupAssetDao(), repository) == BackupProgress.WAITING_FOR_SERVER) {
                    waiting = true
                } else {
                    deleteBackupSnapshot(applicationContext, asset.operationId)
                }
            }
            if (waiting) Result.retry() else Result.success()
        } finally {
            database.close()
        }
    }
}

internal fun isRetryable(statusCode: Int): Boolean = statusCode == 408 || statusCode == 429 || statusCode >= 500

internal fun backupNetworkAllowed(
    allowMobileData: Boolean,
    hasValidatedInternet: Boolean,
    unmetered: Boolean,
): Boolean = hasValidatedInternet && (allowMobileData || unmetered)

fun isBackupNetworkAllowed(context: Context, allowMobileData: Boolean): Boolean {
    val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
    val capabilities = connectivityManager.getNetworkCapabilities(connectivityManager.activeNetwork)
        ?: return false
    return backupNetworkAllowed(
        allowMobileData = allowMobileData,
        hasValidatedInternet = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED),
        unmetered = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
    )
}

fun observeBackupNetworkAllowed(context: Context, allowMobileData: Boolean): Flow<Boolean> =
    callbackFlow {
        val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }

            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }

            override fun onLost(network: Network) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }
        }
        connectivityManager.registerDefaultNetworkCallback(callback)
        trySend(isBackupNetworkAllowed(context, allowMobileData))
        awaitClose { connectivityManager.unregisterNetworkCallback(callback) }
    }.distinctUntilChanged()

enum class BackupMediaAccess {
    FULL,
    PARTIAL,
    DENIED,
}

enum class BackupLocationMetadataAccess {
    PRESERVED,
    DENIED,
}

internal const val IMMEDIATE_BACKUP_WORK_NAME = "momento_backup_now"
internal const val PERIODIC_BACKUP_WORK_NAME = "momento_backup_periodic"
internal const val LEGACY_BACKUP_WORK_NAME = "momento_backup"
private const val BACKUP_CHANNEL = "backup"
private const val BACKUP_CANCELLATION_WORK = "momento_backup_cancellation"
internal const val LOSSLESS_BACKUP_PROTOCOL_VERSION = 2
private const val BACKUP_SNAPSHOT_DIRECTORY = "backup-snapshots"
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

internal fun backupUsesOriginalMediaUri(
    sdkVersion: Int,
    locationMetadataAccess: BackupLocationMetadataAccess,
): Boolean = sdkVersion >= Build.VERSION_CODES.Q &&
    locationMetadataAccess == BackupLocationMetadataAccess.PRESERVED

private fun backupMediaUri(
    uri: Uri,
    sdkVersion: Int,
    locationMetadataAccess: BackupLocationMetadataAccess,
): Uri {
    if (!backupUsesOriginalMediaUri(sdkVersion, locationMetadataAccess)) return uri
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return uri
    return MediaStore.setRequireOriginal(uri)
}

private data class BackupSnapshot(
    val file: File,
    val byteSize: Long,
    val contentHash: String,
    val metadata: JsonObject,
)

private data class MediaStoreMetadata(
    val columns: JsonObject,
    val dateTakenMilliseconds: Long?,
)

private suspend fun prepareBackupSnapshot(
    context: Context,
    asset: BackupAssetEntity,
    locationMetadataAccess: BackupLocationMetadataAccess,
): BackupSnapshot = withContext(Dispatchers.IO) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    if (!snapshotDirectory.isDirectory && !snapshotDirectory.mkdirs()) {
        throw IOException("Could not create the protected backup snapshot directory")
    }
    val snapshotFile = File(snapshotDirectory, "${asset.operationId}.original")
    val metadataFile = File(snapshotDirectory, "${asset.operationId}.metadata.json")
    if (snapshotFile.isFile && snapshotFile.length() != asset.byteSize) {
        Files.delete(snapshotFile.toPath())
        Files.deleteIfExists(metadataFile.toPath())
    }
    if (!snapshotFile.isFile) {
        Files.deleteIfExists(metadataFile.toPath())
        createBackupSnapshotFile(context, asset, locationMetadataAccess, snapshotFile)
    }

    val contentHash = sha256Hex(snapshotFile)
    val metadata = if (metadataFile.isFile) {
        Json.parseToJsonElement(metadataFile.readText()).jsonObject
    } else {
        val mediaStoreMetadata = captureMediaStoreMetadata(context, asset)
        val capturedMetadata = backupMetadataEnvelope(context, asset, contentHash, mediaStoreMetadata)
        writeBackupMetadataFile(metadataFile, capturedMetadata)
        capturedMetadata
    }
    val declaredContentHash = metadata["momentoBackup"]
        ?.jsonObject
        ?.get("contentHash")
        ?.jsonPrimitive
        ?.content
    if (declaredContentHash != contentHash) {
        throw TerminalBackupException("Protected backup metadata does not match its original snapshot")
    }
    BackupSnapshot(
        file = snapshotFile,
        byteSize = snapshotFile.length(),
        contentHash = contentHash,
        metadata = metadata,
    )
}

private fun writeBackupMetadataFile(metadataFile: File, metadata: JsonObject) {
    val pendingFile = File(metadataFile.parentFile, "${metadataFile.name}.pending")
    Files.deleteIfExists(pendingFile.toPath())
    var moved = false
    try {
        FileOutputStream(pendingFile).use { output ->
            output.write(metadata.toString().toByteArray(Charsets.UTF_8))
            output.fd.sync()
        }
        moveSnapshotAtomically(pendingFile, metadataFile)
        moved = true
    } finally {
        if (!moved) Files.deleteIfExists(pendingFile.toPath())
    }
}

private fun createBackupSnapshotFile(
    context: Context,
    asset: BackupAssetEntity,
    locationMetadataAccess: BackupLocationMetadataAccess,
    snapshotFile: File,
) {
    val pendingFile = File(snapshotFile.parentFile, "${asset.operationId}.pending")
    Files.deleteIfExists(pendingFile.toPath())
    var moved = false
    try {
        val mediaUri = backupMediaUri(
            Uri.parse(asset.uri),
            Build.VERSION.SDK_INT,
            locationMetadataAccess,
        )
        val mediaStream = try {
            context.contentResolver.openInputStream(mediaUri)
        } catch (error: FileNotFoundException) {
            throw TerminalBackupException(error.message ?: "Media asset is no longer available")
        } ?: throw TerminalBackupException("Media asset is no longer available")
        var copiedBytes = 0L
        mediaStream.use { input ->
            FileOutputStream(pendingFile).use { output ->
                val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                while (true) {
                    val readBytes = input.read(buffer)
                    if (readBytes < 0) break
                    if (readBytes == 0) continue
                    output.write(buffer, 0, readBytes)
                    copiedBytes += readBytes
                }
                output.fd.sync()
            }
        }
        if (copiedBytes != asset.byteSize) {
            throw TerminalBackupException(
                "Original media changed while the protected snapshot was being created",
            )
        }
        moveSnapshotAtomically(pendingFile, snapshotFile)
        moved = true
    } finally {
        if (!moved) Files.deleteIfExists(pendingFile.toPath())
    }
}

private fun moveSnapshotAtomically(pendingFile: File, snapshotFile: File) {
    try {
        Files.move(
            pendingFile.toPath(),
            snapshotFile.toPath(),
            StandardCopyOption.ATOMIC_MOVE,
            StandardCopyOption.REPLACE_EXISTING,
        )
    } catch (_: AtomicMoveNotSupportedException) {
        Files.move(
            pendingFile.toPath(),
            snapshotFile.toPath(),
            StandardCopyOption.REPLACE_EXISTING,
        )
    }
    val parentDirectory = requireNotNull(snapshotFile.parentFile)
    val directoryDescriptor = Os.open(
        parentDirectory.absolutePath,
        OsConstants.O_RDONLY,
        0,
    )
    try {
        Os.fsync(directoryDescriptor)
    } finally {
        Os.close(directoryDescriptor)
    }
}

private fun captureMediaStoreMetadata(context: Context, asset: BackupAssetEntity): MediaStoreMetadata {
    var dateTakenMilliseconds: Long? = null
    val columns = context.contentResolver.query(Uri.parse(asset.uri), null, null, null, null)?.use { cursor ->
        if (!cursor.moveToFirst()) return@use JsonObject(emptyMap())
        buildJsonObject {
            cursor.columnNames.forEachIndexed { columnIndex, columnName ->
                put(columnName, cursorMetadataValue(cursor, columnIndex))
                if (columnName == MediaStore.MediaColumns.DATE_TAKEN && !cursor.isNull(columnIndex)) {
                    dateTakenMilliseconds = cursor.getLong(columnIndex).takeIf { timestamp -> timestamp > 0 }
                }
            }
        }
    } ?: JsonObject(emptyMap())
    return MediaStoreMetadata(columns, dateTakenMilliseconds)
}

private fun cursorMetadataValue(cursor: Cursor, columnIndex: Int): JsonObject = buildJsonObject {
    when (cursor.getType(columnIndex)) {
        Cursor.FIELD_TYPE_NULL -> {
            put("type", "null")
            put("value", JsonNull)
        }
        Cursor.FIELD_TYPE_INTEGER -> {
            put("type", "integer")
            put("value", cursor.getLong(columnIndex))
        }
        Cursor.FIELD_TYPE_FLOAT -> {
            put("type", "float")
            put("value", cursor.getDouble(columnIndex))
        }
        Cursor.FIELD_TYPE_BLOB -> {
            put("type", "base64")
            put("value", android.util.Base64.encodeToString(cursor.getBlob(columnIndex), android.util.Base64.NO_WRAP))
        }
        else -> {
            put("type", "string")
            put("value", cursor.getString(columnIndex))
        }
    }
}

private fun backupMetadataEnvelope(
    context: Context,
    asset: BackupAssetEntity,
    contentHash: String,
    mediaStoreMetadata: MediaStoreMetadata,
): JsonObject = buildJsonObject {
    put("momentoBackup", buildJsonObject {
        put("schemaVersion", LOSSLESS_BACKUP_PROTOCOL_VERSION)
        put("capturedAt", Instant.now().toString())
        put("source", "androidMediaStore")
        put("sourceUri", asset.uri)
        put("originalFilename", asset.displayName)
        put("mimeType", asset.mimeType)
        put("byteSize", asset.byteSize)
        put("contentHash", contentHash)
        put("sourceModifiedAt", Instant.ofEpochSecond(asset.modifiedAt).toString())
        put("folder", asset.folder)
        put("device", buildJsonObject {
            put("manufacturer", Build.MANUFACTURER)
            put("model", Build.MODEL)
            put("androidSdk", Build.VERSION.SDK_INT)
        })
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            put("mediaStoreVersion", MediaStore.getVersion(context))
            put(
                "mediaStoreVolumeNames",
                JsonArray(
                    MediaStore.getExternalVolumeNames(context)
                        .sorted()
                        .map(::JsonPrimitive),
                ),
            )
        }
        put("mediaStoreColumns", mediaStoreMetadata.columns)
    })
    mediaStoreMetadata.dateTakenMilliseconds?.let { timestamp ->
        put("photoTakenTime", buildJsonObject {
            put("timestamp", (timestamp / 1000L).toString())
        })
    }
}

internal fun sha256Hex(bytes: ByteArray, length: Int): String {
    require(length in 0..bytes.size) { "length must be within the byte array" }
    val digest = MessageDigest.getInstance("SHA-256")
    digest.update(bytes, 0, length)
    return digest.digest().toHexString()
}

private fun sha256Hex(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().buffered().use { input ->
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        while (true) {
            val readBytes = input.read(buffer)
            if (readBytes < 0) break
            if (readBytes > 0) digest.update(buffer, 0, readBytes)
        }
    }
    return digest.digest().toHexString()
}

private fun ByteArray.toHexString(): String = joinToString("") { byte ->
    (byte.toInt() and 0xff).toString(16).padStart(2, '0')
}

private fun deleteBackupSnapshot(context: Context, operationId: String) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.original").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.pending").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.metadata.json").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.metadata.json.pending").toPath())
}

private fun deleteAllBackupSnapshots(context: Context) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    snapshotDirectory.listFiles()?.forEach { snapshot -> Files.deleteIfExists(snapshot.toPath()) }
    Files.deleteIfExists(snapshotDirectory.toPath())
}

fun backupMediaAccessLabel(access: BackupMediaAccess): String = when (access) {
    BackupMediaAccess.FULL -> "All photos and videos are available for backup"
    BackupMediaAccess.PARTIAL -> "Only selected photos or media types are available for backup"
    BackupMediaAccess.DENIED -> "Photo and video access is required before backup can run"
}

fun backupLocationMetadataAccessLabel(access: BackupLocationMetadataAccess): String = when (access) {
    BackupLocationMetadataAccess.PRESERVED -> "Photo location metadata will be preserved"
    BackupLocationMetadataAccess.DENIED -> "Photo location access is required for lossless backup"
}

private fun backupConstraints(allowMobileData: Boolean): Constraints = Constraints.Builder()
    .setRequiredNetworkType(if (allowMobileData) NetworkType.CONNECTED else NetworkType.UNMETERED)
    .build()

private fun ensureBackupNotificationChannel(context: Context) {
    context.getSystemService(NotificationManager::class.java).createNotificationChannel(
        NotificationChannel(BACKUP_CHANNEL, "Momento backup", NotificationManager.IMPORTANCE_LOW),
    )
}

fun scheduleImmediateBackup(context: Context, allowMobileData: Boolean) {
    ensureBackupNotificationChannel(context)
    WorkManager.getInstance(context).enqueueUniqueWork(
        IMMEDIATE_BACKUP_WORK_NAME,
        ExistingWorkPolicy.KEEP,
        OneTimeWorkRequestBuilder<BackupWorker>()
            .setConstraints(backupConstraints(allowMobileData))
            .build(),
    )
}

fun schedulePeriodicBackup(context: Context, allowMobileData: Boolean) {
    ensureBackupNotificationChannel(context)
    val workManager = WorkManager.getInstance(context)
    workManager.cancelUniqueWork(LEGACY_BACKUP_WORK_NAME)
    workManager.enqueueUniquePeriodicWork(
        PERIODIC_BACKUP_WORK_NAME,
        ExistingPeriodicWorkPolicy.UPDATE,
        PeriodicWorkRequestBuilder<BackupWorker>(24, TimeUnit.HOURS)
            .setInitialDelay(24, TimeUnit.HOURS)
            .setConstraints(backupConstraints(allowMobileData))
            .build(),
    )
}

sealed interface BackupHistoryClearResult {
    data class Cleared(val recordCount: Int) : BackupHistoryClearResult
    data object ActiveBackup : BackupHistoryClearResult
}

private suspend fun stopScheduledBackupWork(workManager: WorkManager) {
    workManager.cancelUniqueWork(IMMEDIATE_BACKUP_WORK_NAME).await()
    workManager.cancelUniqueWork(PERIODIC_BACKUP_WORK_NAME).await()
    workManager.cancelUniqueWork(LEGACY_BACKUP_WORK_NAME).await()
}

suspend fun clearBackupHistory(
    context: Context,
    assets: BackupAssetDao,
    settingsStore: SettingsStore,
    allowMobileData: Boolean,
): BackupHistoryClearResult {
    val workManager = WorkManager.getInstance(context)
    stopScheduledBackupWork(workManager)
    if (assets.activeRecordCount() > 0) {
        schedulePeriodicBackup(context, allowMobileData)
        return BackupHistoryClearResult.ActiveBackup
    }

    settingsStore.rotateBackupGeneration()
    val deletedRecords = assets.deleteAll()
    deleteAllBackupSnapshots(context)
    schedulePeriodicBackup(context, allowMobileData)
    return BackupHistoryClearResult.Cleared(deletedRecords)
}

suspend fun requestBackupCancellation(
    context: Context,
    assets: BackupAssetDao,
    allowMobileData: Boolean,
) {
    assets.requestCancellation()
    val workManager = WorkManager.getInstance(context)
    stopScheduledBackupWork(workManager)
    schedulePeriodicBackup(context, allowMobileData)
    val constraints = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
    workManager.enqueueUniqueWork(
        BACKUP_CANCELLATION_WORK,
        ExistingWorkPolicy.REPLACE,
        OneTimeWorkRequestBuilder<BackupCancellationWorker>().setConstraints(constraints).build(),
    )
}
