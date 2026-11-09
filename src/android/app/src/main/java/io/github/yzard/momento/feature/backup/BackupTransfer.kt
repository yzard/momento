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
import io.github.yzard.momento.core.model.BackupBatchCapabilities
import io.github.yzard.momento.core.model.BackupBatchPrepareRequest
import io.github.yzard.momento.core.model.BackupUploadIdsRequest
import io.github.yzard.momento.core.model.BackupCapabilities
import io.github.yzard.momento.core.model.BackupUploadCreateRequest
import io.github.yzard.momento.core.model.BackupUploadResponse
import io.github.yzard.momento.core.network.NetworkClient
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.channels.Channel
import kotlinx.serialization.encodeToString
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


internal enum class BackupProgress { COMPLETED, WAITING_FOR_SERVER, PREPARED }

internal fun backupCanRun(capabilities: BackupCapabilities): Boolean =
    capabilities.enabled && capabilities.protocolVersion == LOSSLESS_BACKUP_PROTOCOL_VERSION

internal class TerminalBackupException(message: String) : Exception(message)

internal fun serverProgress(response: BackupUploadResponse): BackupProgress = when (response.status) {
    "completed", "failed", "cancelled", "expired" -> BackupProgress.COMPLETED
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
        BackupDiagnosticLog.append(applicationContext, "Backup started: work=$id attempt=$runAttemptCount")
        val database = BackupDatabase.create(applicationContext)
        return try {
            val assets = database.backupAssetDao()
            val settingsStore = SettingsStore(applicationContext)
            val tokenStore = EncryptedTokenStore(applicationContext)
            val repository = MomentoRepository(settingsStore, tokenStore, NetworkClient(tokenStore, io.github.yzard.momento.core.cache.MediaCacheStore.get(applicationContext)))
            val settings = settingsStore.settings.first()
            if (!tokenStore.isAuthenticated.value || settings.origin == null) {
                BackupDiagnosticLog.append(applicationContext, "Backup stopped: sign in and configure a server first")
                return Result.failure()
            }
            val mediaAccess = currentBackupMediaAccess(applicationContext)
            val locationMetadataAccess = currentBackupLocationMetadataAccess(applicationContext)
            if (!backupCanReadOriginalMedia(mediaAccess, locationMetadataAccess)) {
                BackupDiagnosticLog.append(applicationContext, "Backup blocked: media=$mediaAccess location=$locationMetadataAccess")
                return Result.success()
            }
            if (!isBackupNetworkAllowed(applicationContext, settings.mobileDataEnabled)) {
                BackupDiagnosticLog.append(applicationContext, "Backup waiting for an allowed network")
                return Result.retry()
            }

            setProgress(androidx.work.workDataOf(BACKUP_PHASE_KEY to "preparing"))
            setForeground(progress("Preparing backup"))
            val deviceId = settingsStore.deviceId()
            repository.registerBackupDevice(deviceId, "${Build.MANUFACTURER} ${Build.MODEL}".trim())
            setProgress(androidx.work.workDataOf(BACKUP_PHASE_KEY to "scanning"))
            MediaStoreScanner(
                applicationContext,
                assets,
                settingsStore.backupGeneration(),
            ).scan(settings.cameraOnly)
            val backupCapabilities = repository.capabilities(settings.origin).backup
            if (!backupCanRun(backupCapabilities)) {
                BackupDiagnosticLog.append(applicationContext, "Backup unavailable: $backupCapabilities")
                return Result.success()
            }
            require(backupCapabilities.maxChunkBytes > 0) { "Server backup chunk limit must be positive" }
            val chunkSize = backupCapabilities.maxChunkBytes.coerceAtMost(8L * 1024L * 1024L).toInt()
            setProgress(androidx.work.workDataOf(BACKUP_PHASE_KEY to "uploading"))
            var waitingForServer = false
            val batch = backupCapabilities.batch?.takeIf { it.version == 1 && it.maxBytes > 0 && it.maxItems > 0 && it.maxManifestBytes > 0 }
            val group = mutableListOf<PreparedBackupUpload>()
            suspend fun flush() {
                if (group.isEmpty()) return
                if (uploadBatch(group.toList(), assets, repository, requireNotNull(batch))) waitingForServer = true
                group.clear()
            }
            if (batch != null) waitingForServer = pollBatchProcessing(assets, repository, batch, settings.cameraOnly)
            for (asset in assets.pending(settings.cameraOnly)) {
                if (batch != null && asset.state == BackupState.SERVER_PROCESSING) continue
                var prepared: PreparedBackupUpload? = null
                val result = transfer(asset, assets, repository, deviceId, chunkSize, locationMetadataAccess,
                    if (batch == null) null else { value -> prepared = value })
                if (result == BackupProgress.PREPARED) {
                    val item = requireNotNull(prepared)
                    val limits = requireNotNull(batch)
                    val fitsBytes = backupBatchCanAppend(group.map { it.snapshot.byteSize }, item.snapshot.byteSize, limits)
                    val fitsJSON = Json.encodeToString(BackupBatchPrepareRequest((group + item).map { it.manifest })).toByteArray(Charsets.UTF_8).size <= limits.maxManifestBytes
                    if (group.isNotEmpty() && (!fitsBytes || !fitsJSON)) flush()
                    val fitsSingleJSON = Json.encodeToString(BackupBatchPrepareRequest(listOf(item.manifest))).toByteArray(Charsets.UTF_8).size <= limits.maxManifestBytes
                    if (item.snapshot.byteSize > limits.maxBytes || !fitsSingleJSON) {
                        if (transfer(asset, assets, repository, deviceId, chunkSize, locationMetadataAccess, null) == BackupProgress.WAITING_FOR_SERVER) waitingForServer = true
                    } else { group.add(item) }
                } else if (result == BackupProgress.WAITING_FOR_SERVER) { waitingForServer = true }
            }
            flush()
            if (batch != null) waitingForServer = pollBatchProcessing(assets, repository, batch, settings.cameraOnly) || waitingForServer
            BackupDiagnosticLog.append(applicationContext, "Backup pass finished: waitingForServer=$waitingForServer")
            if (waitingForServer) Result.retry() else Result.success()
        } catch (error: IOException) {
            BackupDiagnosticLog.append(applicationContext, backupFailureDetail(error))
            Result.retry()
        } catch (error: HttpException) {
            BackupDiagnosticLog.append(applicationContext, backupFailureDetail(error))
            if (isRetryable(error.code()) || error.code() == 409) Result.retry() else Result.failure()
        } catch (error: Exception) {
            if (error is kotlinx.coroutines.CancellationException) throw error
            BackupDiagnosticLog.append(applicationContext, backupFailureDetail(error))
            throw error
        } finally {
            database.close()
        }
    }

    private suspend fun transfer(
        originalAsset: BackupAssetEntity,
        assets: BackupAssetDao,
        repository: BackupRepository,
        deviceId: String,
        chunkSize: Int,
        locationMetadataAccess: BackupLocationMetadataAccess,
        prepareBatch: ((PreparedBackupUpload) -> Unit)?,
    ): BackupProgress {
        var asset = originalAsset
        var phase = "prepare snapshot"
        var activeUploadId = asset.uploadId
        var durableUploadedBytes = asset.uploadedBytes
        var activeProtocolVersion = asset.protocolVersion
        var activeContentHash = asset.contentHash
        try {
            if (asset.state == BackupState.CANCELLING) {
                return cancelBackupAsset(asset, assets, repository)
            }
            setProgress(backupFileProgressData(asset.displayName, asset.uploadedBytes, asset.byteSize))
            var snapshot = prepareBackupSnapshot(applicationContext, asset, locationMetadataAccess)
            activeProtocolVersion = LOSSLESS_BACKUP_PROTOCOL_VERSION
            activeContentHash = snapshot.contentHash
            phase = "create or resume upload"
            var existing = asset.uploadId?.let { repository.backupUploadStatus(it) }
            if (existing?.contentHash != null && existing.contentHash != snapshot.contentHash) {
                throw TerminalBackupException("Server upload does not match this original snapshot")
            }
            if (existing?.status == "expired" || existing?.status == "cancelled") {
                phase = "replace terminated upload"
                if (existing.status == "expired") {
                    val cancelled = repository.cancelBackupUpload(existing.uploadId)
                    if (cancelled.status != "cancelled") throw IOException("Previous backup upload has not stopped")
                }
                val newOperationId = UUID.randomUUID().toString()
                snapshot = rekeyBackupSnapshot(applicationContext, snapshot, asset.operationId, newOperationId)
                if (assets.restartTransfer(asset.uri, asset.operationId, newOperationId) != 1) {
                    rekeyBackupSnapshot(applicationContext, snapshot, newOperationId, asset.operationId)
                    return BackupProgress.WAITING_FOR_SERVER
                }
                asset = asset.copy(operationId = newOperationId, state = BackupState.QUEUED, uploadId = null, uploadedBytes = 0, mediaId = null, errorMessage = null)
                activeUploadId = null
                durableUploadedBytes = 0
                existing = null
            }
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
            if (existing == null && prepareBatch != null) {
                prepareBatch(PreparedBackupUpload(asset, snapshot, createRequest))
                return BackupProgress.PREPARED
            }
            val upload = when {
                existing == null -> repository.createBackupUpload(createRequest)
                existing.contentHash == null -> repository.createBackupUpload(createRequest)
                else -> existing
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

            setProgress(backupFileProgressData(asset.displayName, upload.uploadedSize, snapshot.byteSize))
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
                    phase = "upload chunk $offset-$end/${snapshot.byteSize}"
                    val body = buffer.copyOf(read).toRequestBody(asset.mimeType.toMediaTypeOrNull())
                    val response = transferBackupChunk(
                        uploadId = upload.uploadId,
                        offset = offset,
                        length = read,
                        total = snapshot.byteSize,
                        contentHash = snapshot.contentHash,
                        send = { repository.uploadBackupChunk(upload.uploadId, "bytes $offset-$end/${snapshot.byteSize}", chunkHash, body) },
                        status = { repository.backupUploadStatus(upload.uploadId) },
                        pause = { kotlinx.coroutines.delay(it) },
                        onRetry = { BackupDiagnosticLog.append(applicationContext, "operation=${asset.operationId} upload=${upload.uploadId}: $it") },
                    )
                    offset = response.uploadedSize
                    durableUploadedBytes = offset
                    assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, response.uploadId, response.mediaId, response.error, activeProtocolVersion, activeContentHash)
                    setProgress(backupFileProgressData(asset.displayName, offset, snapshot.byteSize))
                    setForeground(progress(backupFileStatus(asset.displayName, offset, snapshot.byteSize)))
                }
            }
            if (offset != snapshot.byteSize) throw TerminalBackupException("Upload ended before source file")
            phase = "complete upload"
            assets.updateTransfer(asset.uri, BackupState.COMPLETING, offset, upload.uploadId, null, null, activeProtocolVersion, activeContentHash)
            return recordServerStatus(
                asset,
                repository.completeBackupUpload(upload.uploadId),
                assets,
                activeProtocolVersion,
                activeContentHash,
            )
        } catch (error: HttpException) {
            BackupDiagnosticLog.append(applicationContext, "phase=$phase operation=${asset.operationId} upload=$activeUploadId bytes=$durableUploadedBytes: ${backupFailureDetail(error)}")
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, "phase=$phase: ${backupFailureDetail(error)}", activeProtocolVersion, activeContentHash)
            if (isRetryable(error.code()) || error.code() == 409) throw error
            return BackupProgress.COMPLETED
        } catch (error: IOException) {
            BackupDiagnosticLog.append(applicationContext, "phase=$phase operation=${asset.operationId} upload=$activeUploadId bytes=$durableUploadedBytes: ${backupFailureDetail(error)}")
            assets.updateTransfer(asset.uri, BackupState.FAILED, durableUploadedBytes, activeUploadId, asset.mediaId, "phase=$phase: ${backupFailureDetail(error)}", activeProtocolVersion, activeContentHash)
            throw error
        } catch (error: TerminalBackupException) {
            BackupDiagnosticLog.append(applicationContext, "phase=$phase operation=${asset.operationId} upload=$activeUploadId bytes=$durableUploadedBytes: ${backupFailureDetail(error)}")
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, "phase=$phase: ${backupFailureDetail(error)}", activeProtocolVersion, activeContentHash)
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
            BackupDiagnosticLog.append(applicationContext, "phase=$phase operation=${asset.operationId} upload=$activeUploadId bytes=$durableUploadedBytes: ${backupFailureDetail(error)}")
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, "phase=$phase: ${backupFailureDetail(error)}", activeProtocolVersion, activeContentHash)
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
            BackupDiagnosticLog.append(applicationContext, "phase=$phase operation=${asset.operationId} upload=$activeUploadId bytes=$durableUploadedBytes: ${backupFailureDetail(error)}")
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, durableUploadedBytes, activeUploadId, asset.mediaId, "phase=$phase: ${backupFailureDetail(error)}", activeProtocolVersion, activeContentHash)
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
    ): BackupProgress {
        require(response.uploadId.isNotBlank() && (asset.uploadId == null || asset.uploadId == response.uploadId) &&
            response.expectedSize == asset.byteSize && response.uploadedSize in 0..response.expectedSize &&
            (response.contentHash == null || response.contentHash == contentHash) &&
            response.status in setOf("completed", "failed", "cancelled", "expired", "queued", "processing", "uploading", "writing", "completing", "cancelling")) { "Server upload does not match the protected original" }
        return when (response.status) {
        "completed" -> {
            if (!completedBackupHashMatches(protocolVersion, contentHash, response.contentHash)) {
                throw TerminalBackupException("Server completion hash does not match the original snapshot")
            }
            assets.updateTransfer(asset.uri, BackupState.COMPLETED, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, response.contentHash)
            deleteBackupSnapshot(applicationContext, asset.operationId)
            BackupProgress.COMPLETED
        }
        "failed" -> {
            BackupDiagnosticLog.append(applicationContext, "Server failed: operation=${asset.operationId} upload=${response.uploadId}: ${response.error}")
            assets.updateTransfer(asset.uri, BackupState.TERMINAL_FAILED, response.uploadedSize, response.uploadId, response.mediaId, response.error ?: "Server upload ${response.status}", protocolVersion, contentHash)
            deleteBackupSnapshot(applicationContext, asset.operationId)
            BackupProgress.COMPLETED
        }
        "cancelled", "expired" -> {
            assets.updateTransfer(asset.uri, BackupState.CANCELLED, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, contentHash)
            deleteBackupSnapshot(applicationContext, asset.operationId)
            BackupProgress.COMPLETED
        }
        else -> {
            assets.updateTransfer(asset.uri, BackupState.SERVER_PROCESSING, response.uploadedSize, response.uploadId, response.mediaId, response.error, protocolVersion, contentHash)
            BackupProgress.WAITING_FOR_SERVER
        }
    }

    }

    private suspend fun pollBatchProcessing(assets: BackupAssetDao, repository: BackupRepository, limits: BackupBatchCapabilities, cameraOnly: Boolean): Boolean {
        val pending = assets.pending(cameraOnly).filter { it.state == BackupState.SERVER_PROCESSING }
        for (group in pending.chunked(limits.maxItems)) {
            val ids = group.map { requireNotNull(it.uploadId) }
            val results = repository.backupBatchStatus(BackupUploadIdsRequest(ids)).indexed(ids)
            for (asset in group) {
                val item = results.getValue(requireNotNull(asset.uploadId))
                if (item.error != null) {
                    assets.updateTransfer(asset.uri, BackupState.FAILED, asset.uploadedBytes, asset.uploadId, asset.mediaId, item.error.detail, asset.protocolVersion, asset.contentHash)
                } else { recordServerStatus(asset, requireNotNull(item.upload), assets, asset.protocolVersion, asset.contentHash) }
            }
        }
        return assets.pending(cameraOnly).any { it.state == BackupState.SERVER_PROCESSING || it.state == BackupState.UPLOADING || it.state == BackupState.COMPLETING }
    }

    private suspend fun uploadBatch(group: List<PreparedBackupUpload>, assets: BackupAssetDao, repository: BackupRepository, limits: BackupBatchCapabilities): Boolean {
        val accepted = mutableListOf<BackupBatchFile>()
        val prepared = repository.prepareBackupBatch(BackupBatchPrepareRequest(group.map { it.manifest })).indexed(group.map { it.asset.operationId })
        var waiting = false
        for (item in group) {
            val result = prepared.getValue(item.asset.operationId)
            val error = result.error
            if (error != null) {
                assets.updateTransfer(item.asset.uri, BackupState.FAILED, 0, null, null, error.detail, LOSSLESS_BACKUP_PROTOCOL_VERSION, item.snapshot.contentHash)
                if (isRetryable(error.status)) waiting = true
                continue
            }
            val upload = requireNotNull(result.upload)
            require(upload.uploadId.isNotBlank() && upload.expectedSize == item.snapshot.byteSize && upload.uploadedSize in 0..upload.expectedSize &&
                upload.contentHash == item.snapshot.contentHash) { "Server batch does not match the protected original" }
            if (upload.status == "uploading") {
                assets.updateTransfer(item.asset.uri, BackupState.UPLOADING, upload.uploadedSize, upload.uploadId, null, null, LOSSLESS_BACKUP_PROTOCOL_VERSION, item.snapshot.contentHash)
                if (upload.uploadedSize == 0L) accepted.add(BackupBatchFile(upload.uploadId, item)) else waiting = true
            } else if (recordServerStatus(item.asset, upload, assets, LOSSLESS_BACKUP_PROTOCOL_VERSION, item.snapshot.contentHash) == BackupProgress.WAITING_FOR_SERVER) { waiting = true }
        }
        if (accepted.isEmpty()) return waiting
        // All IDs are durable before the request starts. Unknown outcomes reconcile through status on retry.
        try {
            val response = coroutineScope {
                data class FileProgress(val name: String, val sent: Long, val total: Long)
                val events = Channel<FileProgress>(Channel.CONFLATED)
                val monitor = launch {
                    var last = 0L
                    for (event in events) {
                        val now = android.os.SystemClock.elapsedRealtime()
                        if (now - last >= 250 || event.sent == event.total) {
                            last = now
                            setProgress(backupFileProgressData(event.name, event.sent, event.total))
                            setForeground(progress(backupFileStatus(event.name, event.sent, event.total)))
                        }
                    }
                }
                try { repository.uploadBackupBatch(BackupBatchRequestBody(accepted, limits) { name, sent, total -> events.trySend(FileProgress(name, sent, total)) }) }
                finally { events.close(); monitor.join() }
            }
            val results = response.indexed(accepted.map { it.uploadId })
            for ((uploadId, item) in accepted) {
                val result = results.getValue(uploadId)
                if (result.error != null) {
                    assets.updateTransfer(item.asset.uri, BackupState.FAILED, 0, uploadId, null, result.error.detail, LOSSLESS_BACKUP_PROTOCOL_VERSION, item.snapshot.contentHash)
                    waiting = true
                } else if (recordServerStatus(item.asset.copy(uploadId = uploadId), requireNotNull(result.upload), assets, LOSSLESS_BACKUP_PROTOCOL_VERSION, item.snapshot.contentHash) == BackupProgress.WAITING_FOR_SERVER) { waiting = true }
            }
        } catch (error: Exception) {
            if (error is kotlinx.coroutines.CancellationException) throw error
            BackupDiagnosticLog.append(applicationContext, "Batch transfer interrupted: ${backupFailureDetail(error)}")
            for ((_, item) in accepted) {
                assets.recordTransferFailure(item.asset.uri, backupFailureDetail(error))
            }
            // Keep IDs and durable offsets; status reconciliation distinguishes committed files on retry.
            throw error
        }
        return waiting
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

internal const val BACKUP_PHASE_KEY = "backup_phase"
