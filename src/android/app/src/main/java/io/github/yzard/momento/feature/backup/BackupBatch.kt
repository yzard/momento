package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.model.*
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody
import okio.BufferedSink
import java.io.IOException
import java.security.MessageDigest

internal data class PreparedBackupUpload(val asset: BackupAssetEntity, val snapshot: BackupSnapshot, val manifest: BackupUploadCreateRequest)
internal data class BackupBatchFile(val uploadId: String, val prepared: PreparedBackupUpload)

internal fun backupBatchCanAppend(sizes: List<Long>, next: Long, limits: BackupBatchCapabilities): Boolean {
    if (next <= 0 || sizes.size >= limits.maxItems) return false
    if (sizes.isEmpty()) return true
    if (next > limits.maxBytes) return false
    var remaining = limits.maxBytes - next
    for (size in sizes) {
        if (size <= 0 || size > remaining) return false
        remaining -= size
    }
    return true
}

/** Streams protected originals directly; neither files nor the batch are held in memory. */
internal class BackupBatchRequestBody(
    private val files: List<BackupBatchFile>,
    limits: BackupBatchCapabilities,
    private val onProgress: (String, Long, Long) -> Unit,
) : RequestBody() {
    private val manifest = Json.encodeToString(BackupBatchManifest(1, files.map {
        BackupBatchEntry(it.uploadId, it.prepared.snapshot.byteSize, it.prepared.snapshot.contentHash)
    })).toByteArray(Charsets.UTF_8)
    private val length: Long
    init {
        require(files.isNotEmpty() && files.size <= limits.maxItems && manifest.size <= limits.maxManifestBytes)
        var size = 0L
        for (file in files) {
            require(file.prepared.snapshot.byteSize > 0 && file.prepared.snapshot.byteSize <= Long.MAX_VALUE - size)
            size += file.prepared.snapshot.byteSize
        }
        require(files.size == 1 || size <= limits.maxBytes)
        require(size <= Long.MAX_VALUE - manifest.size - 4)
        length = size + manifest.size + 4
    }
    override fun contentType() = "application/octet-stream".toMediaType()
    override fun contentLength() = length
    override fun writeTo(sink: BufferedSink) {
        sink.writeInt(manifest.size)
        sink.write(manifest)
        for ((_, prepared) in files) {
            val hash = MessageDigest.getInstance("SHA-256")
            var sent = 0L
            prepared.snapshot.file.inputStream().use { input ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val read = input.read(buffer)
                    if (read < 0) break
                    if (read.toLong() > prepared.snapshot.byteSize - sent) throw IOException("Protected original changed: ${prepared.asset.displayName}")
                    hash.update(buffer, 0, read)
                    sink.write(buffer, 0, read)
                    sent += read
                    onProgress(prepared.asset.displayName, sent, prepared.snapshot.byteSize)
                }
            }
            val actual = hash.digest().joinToString("") { "%02x".format(it) }
            if (sent != prepared.snapshot.byteSize || actual != prepared.snapshot.contentHash) throw IOException("Protected original changed: ${prepared.asset.displayName}")
        }
    }
}
