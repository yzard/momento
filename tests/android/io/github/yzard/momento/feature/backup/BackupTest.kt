package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupUploadResponse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupTest {
    @Test fun discoveredAssetsStartQueuedWithStableClientId() {
        val asset = discoveredAsset("content://media/42", 42, "IMG_0042.jpg", "image/jpeg", 12, 100, "Camera")
        assertEquals("media_42", asset.clientAssetId)
        assertEquals(BackupState.QUEUED, asset.state)
        assertTrue(asset.operationId.isNotBlank())
    }

    @Test fun retriesOnlyTransientHttpResponses() {
        assertTrue(isRetryable(408))
        assertTrue(isRetryable(429))
        assertTrue(isRetryable(503))
        assertFalse(isRetryable(400))
        assertFalse(isRetryable(401))
    }

    @Test fun serverProcessingIsRetriedWithoutBlockingTheWorker() {
        assertEquals(BackupProgress.WAITING_FOR_SERVER, serverProgress(BackupUploadResponse("upload", "processing", 10, 10, null, null)))
    }

    @Test fun completedAndFailedServerResponsesAreTerminal() {
        assertEquals(BackupProgress.COMPLETED, serverProgress(BackupUploadResponse("upload", "completed", 10, 10, 4, null)))
        assertEquals(BackupProgress.COMPLETED, serverProgress(BackupUploadResponse("upload", "failed", 10, 10, null, "bad file")))
    }
}
