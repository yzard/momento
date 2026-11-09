package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable data class FeatureFlags(val llm: Boolean, val imageTagging: Boolean, val deduplicate: Boolean, val faceDetection: Boolean, val imageAesthetics: Boolean, val screenshotDetection: Boolean, val documentDetection: Boolean)
@Serializable data class BackupCapabilities(val enabled: Boolean, val protocolVersion: Int, val maxUploadBytes: Long, val maxChunkBytes: Long, val sessionExpiryHours: Long, val batch: BackupBatchCapabilities? = null)
@Serializable data class Capabilities(val appVersion: String, val apiVersion: Int, val supportedMediaExtensions: List<String>, val features: FeatureFlags, val backup: BackupCapabilities)
@Serializable data class BackupDeviceRegisterRequest(val deviceId: String, val deviceName: String)
@Serializable data class BackupDeviceRegisterResponse(val registered: Boolean)
@Serializable data class BackupUploadCreateRequest(val protocolVersion: Int, val deviceId: String, val clientAssetId: String, val operationId: String, val originalFilename: String, val mimeType: String, val byteSize: Long, val contentHash: String, val sourceModifiedAt: String, val metadata: JsonObject)
@Serializable data class BackupUploadIdRequest(val uploadId: String)
@Serializable data class BackupUploadResponse(val uploadId: String, val status: String, val uploadedSize: Long, val expectedSize: Long, val contentHash: String?, val mediaId: Long?, val error: String?)

enum class BackupState { QUEUED, UPLOADING, COMPLETING, SERVER_PROCESSING, COMPLETED, FAILED, TERMINAL_FAILED, CANCELLING, CANCELLED }

@Serializable data class BackupBatchCapabilities(val version: Int, val maxBytes: Long, val maxItems: Int, val maxManifestBytes: Int)
@Serializable data class BackupBatchPrepareRequest(val uploads: List<BackupUploadCreateRequest>)
@Serializable data class BackupUploadIdsRequest(val uploadIds: List<String>)
@Serializable data class BackupBatchEntry(val uploadId: String, val byteSize: Long, val contentHash: String)
@Serializable data class BackupBatchManifest(val version: Int, val entries: List<BackupBatchEntry>)
@Serializable data class BackupBatchError(val status: Int, val detail: String)
@Serializable data class BackupBatchResult(val key: String, val upload: BackupUploadResponse?, val error: BackupBatchError?)
@Serializable data class BackupBatchResponse(val results: List<BackupBatchResult>) {
    fun indexed(keys: List<String>): Map<String, BackupBatchResult> {
        require(keys.size == keys.toSet().size && results.size == keys.size && results.map { it.key }.toSet() == keys.toSet() &&
            results.all { (it.upload != null) != (it.error != null) }) { "Server returned an invalid backup batch response" }
        return results.associateBy { it.key }
    }
}
