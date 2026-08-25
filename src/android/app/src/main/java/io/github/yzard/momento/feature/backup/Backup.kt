package io.github.yzard.momento.feature.backup

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ContentUris
import android.content.Context
import android.content.pm.ServiceInfo
import android.net.Uri
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.provider.MediaStore
import androidx.core.app.NotificationCompat
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
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.database.BackupAssetDao
import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupUploadCreateRequest
import io.github.yzard.momento.core.model.BackupUploadResponse
import io.github.yzard.momento.core.network.NetworkClient
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody.Companion.toRequestBody
import retrofit2.HttpException
import java.io.FileNotFoundException
import java.io.IOException
import java.time.Instant
import java.util.UUID
import java.util.concurrent.TimeUnit

class MediaStoreScanner(private val context: Context, private val assets: BackupAssetDao) {
    suspend fun scan(cameraOnly: Boolean) {
        val collection = MediaStore.Files.getContentUri("external")
        val projection = arrayOf(MediaStore.Files.FileColumns._ID, MediaStore.Files.FileColumns.DISPLAY_NAME, MediaStore.Files.FileColumns.MIME_TYPE, MediaStore.Files.FileColumns.SIZE, MediaStore.Files.FileColumns.DATE_MODIFIED, MediaStore.Images.ImageColumns.BUCKET_DISPLAY_NAME)
        val selection = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (?, ?)" + if (cameraOnly) " AND ${MediaStore.Images.ImageColumns.BUCKET_DISPLAY_NAME} = ?" else ""
        val arguments = buildList {
            add(MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString())
            add(MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString())
            if (cameraOnly) add("Camera")
        }.toTypedArray()
        context.contentResolver.query(collection, projection, selection, arguments, "${MediaStore.Files.FileColumns.DATE_MODIFIED} DESC")?.use { cursor ->
            while (cursor.moveToNext()) {
                val mimeType = cursor.getString(2) ?: continue
                val asset = discoveredAsset(ContentUris.withAppendedId(collection, cursor.getLong(0)).toString(), cursor.getLong(0), cursor.getString(1) ?: "media", mimeType, cursor.getLong(3), cursor.getLong(4), cursor.getString(5) ?: "")
                if (assets.insertDiscovered(asset) == -1L) {
                    assets.reconcileDiscovered(asset.uri, asset.clientAssetId, asset.operationId, asset.displayName, asset.mimeType, asset.byteSize, asset.modifiedAt, asset.folder)
                }
            }
        }
    }
}

internal fun discoveredAsset(uri: String, mediaStoreId: Long, displayName: String, mimeType: String, byteSize: Long, modifiedAt: Long, folder: String): BackupAssetEntity =
    BackupAssetEntity(uri, "media_$mediaStoreId", UUID.randomUUID().toString(), displayName, mimeType, byteSize, modifiedAt, folder, BackupState.QUEUED, null, 0, null, null)

internal enum class BackupProgress { COMPLETED, WAITING_FOR_SERVER }

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
            if (!isBackupNetworkAllowed(applicationContext, settings.mobileDataEnabled)) return Result.retry()

            setForeground(progress("Preparing backup"))
            val deviceId = settingsStore.deviceId()
            repository.registerBackupDevice(deviceId, "${Build.MANUFACTURER} ${Build.MODEL}".trim())
            MediaStoreScanner(applicationContext, assets).scan(settings.cameraOnly)
            val chunkSize = repository.capabilities(settings.origin).backup.maxChunkBytes.coerceAtMost(1024L * 1024L).toInt()
            var waitingForServer = false
            for (asset in assets.pending(settings.cameraOnly)) {
                if (transfer(asset, assets, repository, deviceId, chunkSize) == BackupProgress.WAITING_FOR_SERVER) waitingForServer = true
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

    private suspend fun transfer(asset: BackupAssetEntity, assets: BackupAssetDao, repository: MomentoRepository, deviceId: String, chunkSize: Int): BackupProgress {
        var activeUploadId = asset.uploadId
        var durableUploadedBytes = asset.uploadedBytes
        try {
            if (asset.state == BackupState.CANCELLING) {
                return cancelBackupAsset(asset, assets, repository)
            }
            val existing = asset.uploadId?.let { repository.backupUploadStatus(it) }
            if (existing != null) {
                val progress = recordServerStatus(asset, existing, assets)
                if (asset.state == BackupState.SERVER_PROCESSING || progress == BackupProgress.COMPLETED) return progress
                if (asset.state == BackupState.COMPLETING && existing.status != "uploading") return progress
            }

            val upload = existing ?: repository.createBackupUpload(BackupUploadCreateRequest(deviceId, asset.clientAssetId, asset.operationId, asset.displayName, asset.mimeType, asset.byteSize, Instant.ofEpochSecond(asset.modifiedAt).toString()))
            activeUploadId = upload.uploadId
            durableUploadedBytes = upload.uploadedSize
            if (upload.status != "uploading") return recordServerStatus(asset, upload, assets)

            var offset = upload.uploadedSize
            assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, upload.uploadId, null, null)
            val mediaStream = try {
                applicationContext.contentResolver.openInputStream(Uri.parse(asset.uri))
            } catch (error: FileNotFoundException) {
                throw TerminalBackupException(error.message ?: "Media asset is no longer available")
            }
            mediaStream.use { stream ->
                if (stream == null) throw TerminalBackupException("Media asset is no longer available")
                skipFully(stream, offset)
                val buffer = ByteArray(chunkSize)
                while (offset < asset.byteSize) {
                    val read = stream.read(buffer)
                    if (read <= 0) break
                    val end = offset + read - 1
                    val response = repository.uploadBackupChunk(upload.uploadId, "bytes $offset-$end/${asset.byteSize}", buffer.copyOf(read).toRequestBody(asset.mimeType.toMediaTypeOrNull()))
                    offset = response.uploadedSize
                    durableUploadedBytes = offset
                    assets.updateTransfer(asset.uri, BackupState.UPLOADING, offset, response.uploadId, response.mediaId, response.error)
                    setForeground(progress("Uploading ${asset.displayName}"))
                }
            }
            if (offset != asset.byteSize) throw TerminalBackupException("Upload ended before source file")
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

private const val BACKUP_CHANNEL = "backup"
private const val BACKUP_WORK = "momento_backup"
private const val BACKUP_CANCELLATION_WORK = "momento_backup_cancellation"

fun backupReadPermissions(): Array<String> = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) arrayOf(android.Manifest.permission.READ_MEDIA_IMAGES, android.Manifest.permission.READ_MEDIA_VIDEO, android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED) else arrayOf(android.Manifest.permission.READ_MEDIA_IMAGES, android.Manifest.permission.READ_MEDIA_VIDEO)
} else arrayOf(android.Manifest.permission.READ_EXTERNAL_STORAGE)

fun scheduleBackup(context: Context, allowMobileData: Boolean, immediate: Boolean) {
    context.getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel(BACKUP_CHANNEL, "Momento backup", NotificationManager.IMPORTANCE_LOW))
    val constraints = Constraints.Builder().setRequiredNetworkType(if (allowMobileData) NetworkType.CONNECTED else NetworkType.UNMETERED).build()
    val workManager = WorkManager.getInstance(context)
    if (immediate) workManager.enqueueUniqueWork(BACKUP_WORK, ExistingWorkPolicy.KEEP, OneTimeWorkRequestBuilder<BackupWorker>().setConstraints(constraints).build())
    else workManager.enqueueUniquePeriodicWork(BACKUP_WORK, ExistingPeriodicWorkPolicy.UPDATE, PeriodicWorkRequestBuilder<BackupWorker>(24, TimeUnit.HOURS).setConstraints(constraints).build())
}

suspend fun requestBackupCancellation(context: Context, assets: BackupAssetDao) {
    assets.requestCancellation()
    val workManager = WorkManager.getInstance(context)
    workManager.cancelUniqueWork(BACKUP_WORK)
    val constraints = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
    workManager.enqueueUniqueWork(
        BACKUP_CANCELLATION_WORK,
        ExistingWorkPolicy.REPLACE,
        OneTimeWorkRequestBuilder<BackupCancellationWorker>().setConstraints(constraints).build(),
    )
}
