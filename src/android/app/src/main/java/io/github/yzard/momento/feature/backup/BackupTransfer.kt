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
import io.github.yzard.momento.core.data.BackupRepository
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


internal enum class BackupProgress { COMPLETED, WAITING_FOR_SERVER }

internal fun backupCanRun(capabilities: BackupCapabilities): Boolean =
    capabilities.enabled && capabilities.protocolVersion == LOSSLESS_BACKUP_PROTOCOL_VERSION

internal class TerminalBackupException(message: String) : Exception(message)

internal fun serverProgress(response: BackupUploadResponse): BackupProgress = when (response.status) {
    "completed", "failed", "cancelled" -> BackupProgress.COMPLETED
    else -> BackupProgress.WAITING_FOR_SERVER
}

internal fun completedBackupHashMatches(
    protocolVersion: Int,
    expectedContentHash: String?,
    serverContentHash: String?,
): Boolean = protocolVersion == LOSSLESS_BACKUP_PROTOCOL_VERSION &&
    expectedContentHash != null &&
    expectedContentHash == serverContentHash

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
        repository: BackupRepository,
        deviceId: String,
        chunkSize: Int,
        locationMetadataAccess: BackupLocationMetadataAccess,
    ): BackupProgress {
        var activeUploadId = asset.uploadId
        var durableUploadedBytes = asset.uploadedBytes
        var activeProtocolVersion = asset.protocolVersion
        var activeContentHash = asset.contentHash
        try {
            if (asset.state == BackupState.CANCELLING) {
                return cancelBackupAsset(asset, assets, repository)
            }
            val snapshot = prepareBackupSnapshot(applicationContext, asset, locationMetadataAccess)
            activeProtocolVersion = LOSSLESS_BACKUP_PROTOCOL_VERSION
            activeContentHash = snapshot.contentHash
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
                val progress = recordServerStatus(
                    asset,
                    upload,
                    assets,
                    activeProtocolVersion,
                    activeContentHash,
                )
                if (asset.state == BackupState.SERVER_PROCESSING || progress == BackupProgress.COMPLETED) return progress
                if (asset.state == BackupState.COMPLETING && upload.status != "uploading") return progress
            }

            activeUploadId = upload.uploadId
            durableUploadedBytes = upload.uploadedSize
            if (upload.status != "uploading") {
                return recordServerStatus(
                    asset,
                    upload,
                    assets,
                    activeProtocolVersion,
                    activeContentHash,
                )
            }

            var offset = upload.uploadedSize
            assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, upload.uploadId, null, null, activeProtocolVersion, activeContentHash)
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
                    assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, response.uploadId, response.mediaId, response.error, activeProtocolVersion, activeContentHash)
                    setForeground(progress("Uploading ${asset.displayName}"))
                }
            }
            if (offset != snapshot.byteSize) throw TerminalBackupException("Upload ended before source file")
            assets.updateTransfer(asset.uri, BackupState.COMPLETING, offset, upload.uploadId, null, null, activeProtocolVersion, activeContentHash)
            return recordServerStatus(
                asset,
                repository.completeBackupUpload(upload.uploadId),
                assets,
                activeProtocolVersion,
                activeContentHash,
            )
        } catch (error: HttpException) {
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, "HTTP ${error.code()}", activeProtocolVersion, activeContentHash)
            if (isRetryable(error.code())) throw error
            return BackupProgress.COMPLETED
        } catch (error: IOException) {
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, error.message, activeProtocolVersion, activeContentHash)
            throw error
        } catch (error: TerminalBackupException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message, activeProtocolVersion, activeContentHash)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                    protocolVersion = activeProtocolVersion,
                    contentHash = activeContentHash,
                ),
                assets,
                repository,
            )
        } catch (error: SecurityException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message, activeProtocolVersion, activeContentHash)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                    protocolVersion = activeProtocolVersion,
                    contentHash = activeContentHash,
                ),
                assets,
                repository,
            )
        } catch (error: IllegalArgumentException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, error.message, activeProtocolVersion, activeContentHash)
            return cancelBackupAsset(
                asset.copy(
                    state = BackupState.CANCELLING,
                    uploadId = activeUploadId,
                    uploadedBytes = durableUploadedBytes,
                    errorMessage = error.message,
                    protocolVersion = activeProtocolVersion,
                    contentHash = activeContentHash,
                ),
                assets,
                repository,
            )
        }
    }

    private suspend fun recordServerStatus(
        asset: BackupAssetEntity,
        response: BackupUploadResponse,
        assets: BackupAssetDao,
        protocolVersion: Int,
        contentHash: String?,
    ): BackupProgress = when (response.status) {
        "completed" -> {
            if (!completedBackupHashMatches(protocolVersion, contentHash, response.contentHash)) {
                throw TerminalBackupException("Server completion hash does not match the original snapshot")
            }
            assets.updateTransfer(asset.uri, BackupState.COMPLETED, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, response.contentHash)
            BackupProgress.COMPLETED
        }
        "failed" -> {
            assets.updateTransfer(asset.uri, BackupState.TERMINAL_FAILED, response.uploadedSize, response.uploadId, response.mediaId, response.error ?: "Server upload ${response.status}", protocolVersion, contentHash)
            BackupProgress.COMPLETED
        }
        "cancelled" -> {
            assets.updateTransfer(asset.uri, BackupState.CANCELLED, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, contentHash)
            BackupProgress.COMPLETED
        }
        else -> {
            assets.updateTransfer(asset.uri, BackupState.SERVER_PROCESSING, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, contentHash)
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

internal const val IMMEDIATE_BACKUP_WORK_NAME = "momento_backup_now"
internal const val PERIODIC_BACKUP_WORK_NAME = "momento_backup_periodic"
internal const val LEGACY_BACKUP_WORK_NAME = "momento_backup"
internal const val BACKUP_CHANNEL = "backup"
internal const val BACKUP_CANCELLATION_WORK = "momento_backup_cancellation"
internal const val LOSSLESS_BACKUP_PROTOCOL_VERSION = 2
private const val BACKUP_SNAPSHOT_DIRECTORY = "backup-snapshots"
