package io.github.yzard.momento.feature.backup

import androidx.work.Data
import androidx.work.workDataOf
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

internal const val BACKUP_FILE_NAME_KEY = "backup_file_name"
internal const val BACKUP_FILE_UPLOADED_KEY = "backup_file_uploaded"
internal const val BACKUP_FILE_TOTAL_KEY = "backup_file_total"

internal fun backupByteCount(bytes: Long): String {
    val value = bytes.coerceAtLeast(0)
    val gigabytes = value >= 1_000_000_000
    val unit = if (gigabytes) 1_000_000_000L else 1_000_000L
    val hundredths = value / unit * 100 + (value % unit * 100 + unit / 2) / unit
    val whole = hundredths / 100
    val fraction = hundredths % 100
    val number = when {
        fraction == 0L -> "$whole"
        fraction % 10 == 0L -> "$whole.${fraction / 10}"
        else -> "$whole.${fraction.toString().padStart(2, '0')}"
    }

    return "$number ${if (gigabytes) "GB" else "MB"}"
}

internal fun backupFileStatus(filename: String, uploaded: Long, total: Long): String =
    "Backing up $filename ${backupByteCount(uploaded)}/${backupByteCount(total)}"

internal fun backupUploadFailure(filename: String, reason: String): String =
    "$filename: Upload failed — ${backupUploadReason(reason)}"

internal fun backupUploadReason(detail: String): String {
    if ("Server response:" in detail) {
        val response = detail.substringAfter("Server response:").trim()
        val message = try {
            val body = Json.parseToJsonElement(response) as? JsonObject
            (body?.get("detail") as? JsonPrimitive)?.content?.takeIf { it.isNotBlank() }
        } catch (_: SerializationException) { null }
        if (message != null) return message
    }
    val firstLine = detail.lineSequence().firstOrNull { it.isNotBlank() }
        ?: return "Unknown upload error"
    return firstLine.replace(Regex("^(?:[\\w$]+\\.)*[\\w$]+(?:Exception|Error):\\s*"), "")
}

internal fun backupFileProgressData(filename: String, uploaded: Long, total: Long): Data = workDataOf(
    BACKUP_PHASE_KEY to "uploading",
    BACKUP_FILE_NAME_KEY to filename,
    BACKUP_FILE_UPLOADED_KEY to uploaded,
    BACKUP_FILE_TOTAL_KEY to total,
)

internal fun backupFileStatus(progress: Data): String? = progress.getString(BACKUP_FILE_NAME_KEY)
    ?.takeIf { it.isNotBlank() }
    ?.let { backupFileStatus(it, progress.getLong(BACKUP_FILE_UPLOADED_KEY, 0), progress.getLong(BACKUP_FILE_TOTAL_KEY, 0)) }
