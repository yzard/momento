package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.data.BackupRepository
import io.github.yzard.momento.core.database.BackupAssetDao
import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupUploadResponse
import java.lang.reflect.Proxy
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BackupCancellationTest {
    @Test fun missingSourceKeepsItsReasonWhenThereIsNoServerUpload() = runBlocking {
        val reason = "No item at content://media/external_primary/file/1490?requireOriginal=1"
        val result = cancel(asset(null, reason), null)
        assertEquals(BackupProgress.COMPLETED, result.progress)
        assertEquals(BackupState.CANCELLED, result.state)
        assertEquals(reason, result.error)
    }

    @Test fun cancellationWithoutAnErrorStillRecordsAReason() = runBlocking {
        val result = cancel(asset(null, null), null)
        assertEquals(BackupState.CANCELLED, result.state)
        assertEquals("Backup cancelled before completion", result.error)
    }

    @Test fun serverCancellationAndExpiryKeepTheOriginalReason() = runBlocking {
        for (status in listOf("cancelled", "expired")) {
            val result = cancel(asset("upload", "Source is no longer available"), response(status, null))
            assertEquals(BackupState.CANCELLED, result.state)
            assertEquals("Source is no longer available", result.error)
        }
        assertEquals(
            "Upload expired before backup completed",
            cancel(asset("upload", null), response("expired", null)).error,
        )
    }

    @Test fun serverErrorTakesPrecedenceOverTheCancellationReason() = runBlocking {
        val result = cancel(asset("upload", "Original cancellation reason"), response("failed", "Stored file is corrupt"))
        assertEquals(BackupState.TERMINAL_FAILED, result.state)
        assertEquals("Stored file is corrupt", result.error)
    }

    @Test fun waitingForServerKeepsTheReasonUntilCancellationFinishes() = runBlocking {
        for (status in listOf("processing", "uploading")) {
            val result = cancel(asset("upload", "Source is no longer available"), response(status, null))
            assertEquals(BackupProgress.WAITING_FOR_SERVER, result.progress)
            assertEquals("Source is no longer available", result.error)
        }
    }

    @Test fun verifiedCompletionClearsTheOldCancellationError() = runBlocking {
        val result = cancel(asset("upload", "Previous error"), response("completed", null))
        assertEquals(BackupProgress.COMPLETED, result.progress)
        assertEquals(BackupState.COMPLETED, result.state)
        assertNull(result.error)
    }

    private fun asset(uploadId: String?, error: String?): BackupAssetEntity = discoveredAsset(
        "content://media/1490", 1490, "external_primary", null,
        "IMG_1490.jpg", "image/jpeg", 100, 100, 0, "Camera",
    ).copy(state = BackupState.CANCELLING, uploadId = uploadId, errorMessage = error, protocolVersion = 2, contentHash = "a".repeat(64))

    private fun response(status: String, error: String?): BackupUploadResponse =
        BackupUploadResponse("upload", status, 100, 100, "a".repeat(64), if (status == "completed") 42 else null, error)

    private data class Outcome(val progress: BackupProgress, val state: BackupState, val error: String?)

    private suspend fun cancel(asset: BackupAssetEntity, response: BackupUploadResponse?): Outcome {
        val transfers = mutableListOf<Pair<BackupState, String?>>()
        val dao = Proxy.newProxyInstance(
            BackupAssetDao::class.java.classLoader, arrayOf(BackupAssetDao::class.java),
        ) { _, method, arguments ->
            check(method.name == "updateTransfer") { "Unexpected database operation: ${method.name}" }
            transfers.add((arguments[1] as BackupState) to (arguments[5] as String?))
            Unit
        } as BackupAssetDao
        val repository = Proxy.newProxyInstance(
            BackupRepository::class.java.classLoader, arrayOf(BackupRepository::class.java),
        ) { _, method, arguments ->
            check(method.name == "cancelBackupUpload" && response != null) { "Unexpected server operation: ${method.name}" }
            assertEquals(asset.uploadId, arguments[0])
            response
        } as BackupRepository
        val progress = cancelBackupAsset(asset, dao, repository)
        val (state, error) = transfers.single()
        return Outcome(progress, state, error)
    }
}
