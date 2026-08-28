package io.github.yzard.momento.feature.backup

import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import android.system.Os
import android.system.OsConstants
import io.github.yzard.momento.core.database.BackupAssetEntity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.io.File
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.time.Instant

private const val BACKUP_SNAPSHOT_DIRECTORY = "backup-snapshots"

internal fun backupUsesOriginalMediaUri(
    sdkVersion: Int,
    locationMetadataAccess: BackupLocationMetadataAccess,
): Boolean = sdkVersion >= Build.VERSION_CODES.Q &&
    locationMetadataAccess == BackupLocationMetadataAccess.PRESERVED

private fun backupMediaUri(
    uri: Uri,
    sdkVersion: Int,
    locationMetadataAccess: BackupLocationMetadataAccess,
): Uri {
    if (!backupUsesOriginalMediaUri(sdkVersion, locationMetadataAccess)) return uri
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return uri
    return MediaStore.setRequireOriginal(uri)
}

internal data class BackupSnapshot(
    val file: File,
    val byteSize: Long,
    val contentHash: String,
    val metadata: JsonObject,
)

private data class MediaStoreMetadata(
    val columns: JsonObject,
    val dateTakenMilliseconds: Long?,
)

internal suspend fun prepareBackupSnapshot(
    context: Context,
    asset: BackupAssetEntity,
    locationMetadataAccess: BackupLocationMetadataAccess,
): BackupSnapshot = withContext(Dispatchers.IO) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    if (!snapshotDirectory.isDirectory && !snapshotDirectory.mkdirs()) {
        throw IOException("Could not create the protected backup snapshot directory")
    }
    val snapshotFile = File(snapshotDirectory, "${asset.operationId}.original")
    val metadataFile = File(snapshotDirectory, "${asset.operationId}.metadata.json")
    if (snapshotFile.isFile && snapshotFile.length() != asset.byteSize) {
        Files.delete(snapshotFile.toPath())
        Files.deleteIfExists(metadataFile.toPath())
    }
    if (!snapshotFile.isFile) {
        Files.deleteIfExists(metadataFile.toPath())
        createBackupSnapshotFile(context, asset, locationMetadataAccess, snapshotFile)
    }

    val contentHash = sha256Hex(snapshotFile)
    val metadata = if (metadataFile.isFile) {
        Json.parseToJsonElement(metadataFile.readText()).jsonObject
    } else {
        val mediaStoreMetadata = captureMediaStoreMetadata(context, asset)
        val capturedMetadata = backupMetadataEnvelope(context, asset, contentHash, mediaStoreMetadata)
        writeBackupMetadataFile(metadataFile, capturedMetadata)
        capturedMetadata
    }
    val declaredContentHash = metadata["momentoBackup"]
        ?.jsonObject
        ?.get("contentHash")
        ?.jsonPrimitive
        ?.content
    if (declaredContentHash != contentHash) {
        throw TerminalBackupException("Protected backup metadata does not match its original snapshot")
    }
    BackupSnapshot(
        file = snapshotFile,
        byteSize = snapshotFile.length(),
        contentHash = contentHash,
        metadata = metadata,
    )
}

private fun writeBackupMetadataFile(metadataFile: File, metadata: JsonObject) {
    val pendingFile = File(metadataFile.parentFile, "${metadataFile.name}.pending")
    Files.deleteIfExists(pendingFile.toPath())
    var moved = false
    try {
        FileOutputStream(pendingFile).use { output ->
            output.write(metadata.toString().toByteArray(Charsets.UTF_8))
            output.fd.sync()
        }
        moveSnapshotAtomically(pendingFile, metadataFile)
        moved = true
    } finally {
        if (!moved) Files.deleteIfExists(pendingFile.toPath())
    }
}

private fun createBackupSnapshotFile(
    context: Context,
    asset: BackupAssetEntity,
    locationMetadataAccess: BackupLocationMetadataAccess,
    snapshotFile: File,
) {
    val pendingFile = File(snapshotFile.parentFile, "${asset.operationId}.pending")
    Files.deleteIfExists(pendingFile.toPath())
    var moved = false
    try {
        val mediaUri = backupMediaUri(
            Uri.parse(asset.uri),
            Build.VERSION.SDK_INT,
            locationMetadataAccess,
        )
        val mediaStream = try {
            context.contentResolver.openInputStream(mediaUri)
        } catch (error: FileNotFoundException) {
            throw TerminalBackupException(error.message ?: "Media asset is no longer available")
        } ?: throw TerminalBackupException("Media asset is no longer available")
        var copiedBytes = 0L
        mediaStream.use { input ->
            FileOutputStream(pendingFile).use { output ->
                val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                while (true) {
                    val readBytes = input.read(buffer)
                    if (readBytes < 0) break
                    if (readBytes == 0) continue
                    output.write(buffer, 0, readBytes)
                    copiedBytes += readBytes
                }
                output.fd.sync()
            }
        }
        if (copiedBytes != asset.byteSize) {
            throw TerminalBackupException(
                "Original media changed while the protected snapshot was being created",
            )
        }
        moveSnapshotAtomically(pendingFile, snapshotFile)
        moved = true
    } finally {
        if (!moved) Files.deleteIfExists(pendingFile.toPath())
    }
}

private fun moveSnapshotAtomically(pendingFile: File, snapshotFile: File) {
    try {
        Files.move(
            pendingFile.toPath(),
            snapshotFile.toPath(),
            StandardCopyOption.ATOMIC_MOVE,
            StandardCopyOption.REPLACE_EXISTING,
        )
    } catch (_: AtomicMoveNotSupportedException) {
        Files.move(
            pendingFile.toPath(),
            snapshotFile.toPath(),
            StandardCopyOption.REPLACE_EXISTING,
        )
    }
    val parentDirectory = requireNotNull(snapshotFile.parentFile)
    val directoryDescriptor = Os.open(
        parentDirectory.absolutePath,
        OsConstants.O_RDONLY,
        0,
    )
    try {
        Os.fsync(directoryDescriptor)
    } finally {
        Os.close(directoryDescriptor)
    }
}

private fun captureMediaStoreMetadata(context: Context, asset: BackupAssetEntity): MediaStoreMetadata {
    var dateTakenMilliseconds: Long? = null
    val columns = context.contentResolver.query(Uri.parse(asset.uri), null, null, null, null)?.use { cursor ->
        if (!cursor.moveToFirst()) return@use JsonObject(emptyMap())
        buildJsonObject {
            cursor.columnNames.forEachIndexed { columnIndex, columnName ->
                put(columnName, cursorMetadataValue(cursor, columnIndex))
                if (columnName == MediaStore.MediaColumns.DATE_TAKEN && !cursor.isNull(columnIndex)) {
                    dateTakenMilliseconds = cursor.getLong(columnIndex).takeIf { timestamp -> timestamp > 0 }
                }
            }
        }
    } ?: JsonObject(emptyMap())
    return MediaStoreMetadata(columns, dateTakenMilliseconds)
}

private fun cursorMetadataValue(cursor: Cursor, columnIndex: Int): JsonObject = buildJsonObject {
    when (cursor.getType(columnIndex)) {
        Cursor.FIELD_TYPE_NULL -> {
            put("type", "null")
            put("value", JsonNull)
        }
        Cursor.FIELD_TYPE_INTEGER -> {
            put("type", "integer")
            put("value", cursor.getLong(columnIndex))
        }
        Cursor.FIELD_TYPE_FLOAT -> {
            put("type", "float")
            put("value", cursor.getDouble(columnIndex))
        }
        Cursor.FIELD_TYPE_BLOB -> {
            put("type", "base64")
            put("value", android.util.Base64.encodeToString(cursor.getBlob(columnIndex), android.util.Base64.NO_WRAP))
        }
        else -> {
            put("type", "string")
            put("value", cursor.getString(columnIndex))
        }
    }
}

private fun backupMetadataEnvelope(
    context: Context,
    asset: BackupAssetEntity,
    contentHash: String,
    mediaStoreMetadata: MediaStoreMetadata,
): JsonObject = buildJsonObject {
    put("momentoBackup", buildJsonObject {
        put("schemaVersion", LOSSLESS_BACKUP_PROTOCOL_VERSION)
        put("capturedAt", Instant.now().toString())
        put("source", "androidMediaStore")
        put("sourceUri", asset.uri)
        put("originalFilename", asset.displayName)
        put("mimeType", asset.mimeType)
        put("byteSize", asset.byteSize)
        put("contentHash", contentHash)
        put("sourceModifiedAt", Instant.ofEpochSecond(asset.modifiedAt).toString())
        put("folder", asset.folder)
        put("device", buildJsonObject {
            put("manufacturer", Build.MANUFACTURER)
            put("model", Build.MODEL)
            put("androidSdk", Build.VERSION.SDK_INT)
        })
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            put("mediaStoreVersion", MediaStore.getVersion(context))
            put(
                "mediaStoreVolumeNames",
                JsonArray(
                    MediaStore.getExternalVolumeNames(context)
                        .sorted()
                        .map(::JsonPrimitive),
                ),
            )
        }
        put("mediaStoreColumns", mediaStoreMetadata.columns)
    })
    mediaStoreMetadata.dateTakenMilliseconds?.let { timestamp ->
        put("photoTakenTime", buildJsonObject {
            put("timestamp", (timestamp / 1000L).toString())
        })
    }
}

internal fun sha256Hex(bytes: ByteArray, length: Int): String {
    require(length in 0..bytes.size) { "length must be within the byte array" }
    val digest = MessageDigest.getInstance("SHA-256")
    digest.update(bytes, 0, length)
    return digest.digest().toHexString()
}

private fun sha256Hex(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().buffered().use { input ->
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        while (true) {
            val readBytes = input.read(buffer)
            if (readBytes < 0) break
            if (readBytes > 0) digest.update(buffer, 0, readBytes)
        }
    }
    return digest.digest().toHexString()
}

private fun ByteArray.toHexString(): String = joinToString("") { byte ->
    (byte.toInt() and 0xff).toString(16).padStart(2, '0')
}

internal fun deleteBackupSnapshot(context: Context, operationId: String) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.original").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.pending").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.metadata.json").toPath())
    Files.deleteIfExists(File(snapshotDirectory, "$operationId.metadata.json.pending").toPath())
}

internal fun deleteAllBackupSnapshots(context: Context) {
    val snapshotDirectory = File(context.noBackupFilesDir, BACKUP_SNAPSHOT_DIRECTORY)
    snapshotDirectory.listFiles()?.forEach { snapshot -> Files.deleteIfExists(snapshot.toPath()) }
    Files.deleteIfExists(snapshotDirectory.toPath())
}

