package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.BackupDeviceRegisterRequest
import io.github.yzard.momento.core.model.BackupDeviceRegisterResponse
import io.github.yzard.momento.core.model.BackupUploadCreateRequest
import io.github.yzard.momento.core.model.BackupUploadIdRequest
import io.github.yzard.momento.core.model.BackupUploadResponse
import okhttp3.RequestBody
import retrofit2.http.Body
import retrofit2.http.Header
import retrofit2.http.POST
import retrofit2.http.PUT
import retrofit2.http.Path

interface BackupApi {
    @POST("api/v1/backup/device/register") suspend fun registerDevice(@Body request: BackupDeviceRegisterRequest): BackupDeviceRegisterResponse
    @POST("api/v1/backup/upload/create") suspend fun createUpload(@Body request: BackupUploadCreateRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/status") suspend fun uploadStatus(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @PUT("api/v1/backup/upload/chunk/{uploadId}") suspend fun uploadChunk(@Path("uploadId") uploadId: String, @Header("Content-Range") range: String, @Header("X-Content-SHA256") contentHash: String, @Body bytes: RequestBody): BackupUploadResponse
    @POST("api/v1/backup/upload/complete") suspend fun completeUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/cancel") suspend fun cancelUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
}
