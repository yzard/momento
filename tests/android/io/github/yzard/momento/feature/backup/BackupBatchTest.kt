package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.model.*
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import okio.Buffer
import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class BackupBatchTest {
    private val limits = BackupBatchCapabilities(1, 50_000_000, 256, 1_048_576)

    @Test fun packsSmallFilesWithoutOverflowAndKeepsLargeOriginalAlone() {
        assertTrue(backupBatchCanAppend(emptyList(), 70_000_000, limits))
        assertFalse(backupBatchCanAppend(listOf(70_000_000), 1, limits))
        assertTrue(backupBatchCanAppend(listOf(20_000_000), 30_000_000, limits))
        assertFalse(backupBatchCanAppend(listOf(20_000_000), 30_000_001, limits))
        assertFalse(backupBatchCanAppend(emptyList(), 0, limits))
        assertFalse(backupBatchCanAppend(List(256) { 1L }, 1, limits))
        assertFalse(backupBatchCanAppend(listOf(Long.MAX_VALUE), Long.MAX_VALUE, limits))
    }

    @Test fun streamsExactOriginalBytesAndDetectsChangedSnapshots() {
        val root = Files.createTempDirectory("backup-batch").toFile()
        try {
            val files = listOf("first-original", "second-original").mapIndexed { index, text ->
                val bytes = text.toByteArray()
                val file = java.io.File(root, "$index.original").apply { writeBytes(bytes) }
                val asset = discoveredAsset("content://media/$index", index.toLong(), "external", null, "$index.jpg", "image/jpeg", bytes.size.toLong(), 100, 0, "Camera")
                val snapshot = BackupSnapshot(file, bytes.size.toLong(), sha256Hex(bytes, bytes.size), JsonObject(emptyMap()))
                val manifest = BackupUploadCreateRequest(2, "device", asset.clientAssetId, asset.operationId, asset.displayName, asset.mimeType, snapshot.byteSize, snapshot.contentHash, "2026-01-01T00:00:00Z", snapshot.metadata)
                BackupBatchFile("upload-$index", PreparedBackupUpload(asset, snapshot, manifest))
            }
            val progress = mutableListOf<String>()
            val body = BackupBatchRequestBody(files, limits) { name, _, _ -> progress.add(name) }
            val output = Buffer()
            body.writeTo(output)
            assertEquals(body.contentLength(), output.size)
            val header = Json.decodeFromString<BackupBatchManifest>(output.readUtf8(output.readInt().toLong()))
            assertEquals(listOf("upload-0", "upload-1"), header.entries.map { it.uploadId })
            assertEquals("first-originalsecond-original", output.readUtf8())
            assertEquals(listOf("0.jpg", "1.jpg"), progress)
            files[1].prepared.snapshot.file.writeText("changed")
            assertThrows(java.io.IOException::class.java) { body.writeTo(Buffer()) }
        } finally { root.deleteRecursively() }
    }

    @Test fun rejectsDuplicateMissingOrAmbiguousResponses() {
        val upload = BackupUploadResponse("upload", "uploading", 0, 1, "hash", null, null)
        for (response in listOf(BackupBatchResponse(emptyList()),
            BackupBatchResponse(listOf(BackupBatchResult("one", upload, null), BackupBatchResult("one", upload, null))),
            BackupBatchResponse(listOf(BackupBatchResult("one", null, null))),
            BackupBatchResponse(listOf(BackupBatchResult("one", upload, BackupBatchError(400, "bad")))))) {
            assertThrows(IllegalArgumentException::class.java) { response.indexed(listOf("one")) }
        }
    }
}
