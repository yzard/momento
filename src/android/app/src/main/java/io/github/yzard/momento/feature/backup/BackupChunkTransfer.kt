package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.model.BackupUploadResponse
import retrofit2.HttpException
import java.io.IOException

/** Retry only this chunk, reconciling a lost acknowledgement with the durable server offset. */
internal suspend fun transferBackupChunk(
    uploadId: String,
    offset: Long,
    length: Int,
    total: Long,
    contentHash: String,
    send: suspend () -> BackupUploadResponse,
    status: suspend () -> BackupUploadResponse,
    pause: suspend (Long) -> Unit,
    onRetry: suspend (String) -> Unit,
): BackupUploadResponse {
    require(offset >= 0 && length > 0 && offset <= total && length.toLong() <= total - offset)
    val end = offset + length
    fun validate(response: BackupUploadResponse) {
        if (response.uploadId != uploadId || response.expectedSize != total ||
            response.contentHash != contentHash || response.status != "uploading" ||
            response.uploadedSize !in listOf(offset, end)
        ) throw IOException("Unexpected backup acknowledgement: status=${response.status} offset=${response.uploadedSize}, expected=$offset or $end")
    }
    for (attempt in 0..3) {
        try {
            val response = send()
            validate(response)
            if (response.uploadedSize != end) throw IOException("Backup chunk was not acknowledged")
            return response
        } catch (error: Exception) {
            if (error !is IOException && (error !is HttpException || (error.code() != 409 && !isRetryable(error.code())))) throw error
            onRetry("chunk offset=$offset length=$length attempt=${attempt + 1}: ${backupFailureDetail(error)}")
            if (attempt == 3) {
                if (error is HttpException && error.code() == 409) throw IOException("Backup chunk is still busy after retries", error)
                throw error
            }
            pause(1_000L shl attempt)
            // If this query fails, the worker retries later from the persisted upload ID.
            val response = status()
            validate(response)
            if (response.uploadedSize == end) return response
        }
    }
    error("Unreachable chunk retry state")
}
