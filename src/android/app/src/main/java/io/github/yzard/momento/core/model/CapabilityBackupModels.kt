package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable data class FeatureFlags(val llm: Boolean, val imageTagging: Boolean, val deduplicate: Boolean, val faceDetection: Boolean, val imageAesthetics: Boolean, val screenshotDetection: Boolean, val documentDetection: Boolean)
@Serializable data class BackupCapabilities(val enabled: Boolean, val protocolVersion: Int, val maxUploadBytes: Long, val maxChunkBytes: Long, val maxActiveUploadsPerUser: Int, val sessionExpiryHours: Long)
@Serializable data class Capabilities(val appVersion: String, val apiVersion: Int, val supportedMediaExtensions: List<String>, val features: FeatureFlags, val backup: BackupCapabilities)
@Serializable data class BackupDeviceRegisterRequest(val deviceId: String, val deviceName: String)
@Serializable data class BackupDeviceRegisterResponse(val registered: Boolean)
@Serializable data class BackupUploadCreateRequest(val protocolVersion: Int, val deviceId: String, val clientAssetId: String, val operationId: String, val originalFilename: String, val mimeType: String, val byteSize: Long, val contentHash: String, val sourceModifiedAt: String, val metadata: JsonObject)
@Serializable data class BackupUploadIdRequest(val uploadId: String)
@Serializable data class BackupUploadResponse(val uploadId: String, val status: String, val uploadedSize: Long, val expectedSize: Long, val contentHash: String?, val mediaId: Long?, val error: String?)

enum class BackupState { QUEUED, UPLOADING, COMPLETING, SERVER_PROCESSING, COMPLETED, FAILED, TERMINAL_FAILED, CANCELLING, CANCELLED }

