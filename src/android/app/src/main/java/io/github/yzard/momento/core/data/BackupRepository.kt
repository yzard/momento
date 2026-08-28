package io.github.yzard.momento.core.data

import io.github.yzard.momento.core.model.BackupDeviceRegisterResponse
import io.github.yzard.momento.core.model.BackupUploadCreateRequest
import io.github.yzard.momento.core.model.BackupUploadResponse
import io.github.yzard.momento.core.model.Capabilities
import okhttp3.RequestBody

interface BackupRepository {
    suspend fun capabilities(origin: String): Capabilities
    suspend fun registerBackupDevice(deviceId: String, deviceName: String): BackupDeviceRegisterResponse
    suspend fun createBackupUpload(request: BackupUploadCreateRequest): BackupUploadResponse
    suspend fun backupUploadStatus(uploadId: String): BackupUploadResponse
    suspend fun uploadBackupChunk(
        uploadId: String,
        range: String,
        contentHash: String,
        body: RequestBody,
    ): BackupUploadResponse
    suspend fun completeBackupUpload(uploadId: String): BackupUploadResponse
    suspend fun cancelBackupUpload(uploadId: String): BackupUploadResponse
}
