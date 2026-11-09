package io.github.yzard.momento.core.database

import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.yzard.momento.core.model.BackupState
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BackupDatabaseInstrumentedTest {
    @Test
    fun uploadFailuresRetainOriginalNamesAndFilterCancelledOrExcludedMedia() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(backupAsset(1, BackupState.FAILED).copy(displayName = "Original.MOV", errorMessage = "Cannot Open"))
            assets.insertDiscovered(backupAsset(2, BackupState.TERMINAL_FAILED).copy(displayName = "Imported.png", folder = "Downloads", errorMessage = "Unsupported original"))
            assets.insertDiscovered(backupAsset(3, BackupState.CANCELLED).copy(errorMessage = "Cancelled by user"))
            assertEquals(listOf("Imported.png", "Original.MOV"), assets.observeUploadFailures(false).first().map { it.displayName })
            assertEquals("Cannot Open", assets.observeUploadFailures(true).first().single().errorMessage)
            assertEquals("Original.MOV", assets.observeUploadFailures(true).first().single().displayName)
        } finally { database.close() }
    }

    @Test
    fun restartUsesNewOperationAndCannotOverrideCancellation() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            val original = backupAsset(9, BackupState.SERVER_PROCESSING).copy(uploadId = "expired-upload", uploadedBytes = 5L * 1024 * 1024 * 1024)
            assets.insertDiscovered(original)
            assertEquals(1, assets.restartTransfer(original.uri, original.operationId, "fresh-operation"))
            val fresh = assets.pending(false).single()
            assertEquals(original.clientAssetId, fresh.clientAssetId)
            assertEquals("fresh-operation", fresh.operationId)
            assertEquals(BackupState.QUEUED, fresh.state)
            assertEquals(0L, fresh.uploadedBytes)
            assertNull(fresh.uploadId)
            assertEquals(0, assets.restartTransfer(original.uri, original.operationId, "stale-operation"))
            assets.requestCancellation()
            assertEquals(0, assets.restartTransfer(original.uri, "fresh-operation", "overwritten-cancellation"))
            assertEquals(BackupState.CANCELLING, assets.pending(false).single().state)
        } finally {
            database.close()
        }
    }

    @Test
    fun cancelledAssetIsRequeuedWhenTheNextScanFindsIt() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(
                BackupAssetEntity(
                    uri = "content://media/42",
                    volumeName = "external",
                    clientAssetId = "media_42",
                    operationId = "cancelled-operation",
                    displayName = "IMG_0042.jpg",
                    mimeType = "image/jpeg",
                    byteSize = 100,
                    modifiedAt = 1_700_000_000,
                    generationModified = 7,
                    folder = "Camera",
                    state = BackupState.CANCELLED,
                    uploadId = "upload-42",
                    uploadedBytes = 50,
                    mediaId = null,
                    errorMessage = "Cancelled by user",
                    protocolVersion = 1,
                    contentHash = null,
                ),
            )

            assets.reconcileDiscovered(
                uri = "content://media/42",
                volumeName = "external",
                clientAssetId = "media_42",
                operationId = "daily-operation",
                displayName = "IMG_0042.jpg",
                mimeType = "image/jpeg",
                byteSize = 100,
                modifiedAt = 1_700_000_000,
                generationModified = 7,
                folder = "Camera",
            )

            val requeuedAsset = assets.pending(cameraOnly = true).single()
            assertEquals(BackupState.QUEUED, requeuedAsset.state)
            assertEquals("daily-operation", requeuedAsset.operationId)
            assertEquals(0, requeuedAsset.uploadedBytes)
            assertNull(requeuedAsset.uploadId)
            assertNull(requeuedAsset.errorMessage)
        } finally {
            database.close()
        }
    }

    @Test
    fun changedMediaStoreGenerationRequeuesACompletedAsset() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(backupAsset(8, BackupState.COMPLETED))

            assets.reconcileDiscovered(
                uri = "content://media/8",
                volumeName = "external",
                clientAssetId = "media_8",
                operationId = "replacement-operation",
                displayName = "IMG_8.jpg",
                mimeType = "image/jpeg",
                byteSize = 100,
                modifiedAt = 1_700_000_008L,
                generationModified = 99,
                folder = "Camera",
            )

            val requeuedAsset = assets.pending(cameraOnly = true).single()
            assertEquals(BackupState.QUEUED, requeuedAsset.state)
            assertEquals(99, requeuedAsset.generationModified)
            assertEquals("replacement-operation", requeuedAsset.operationId)
            assertNull(requeuedAsset.uploadId)
        } finally {
            database.close()
        }
    }

    @Test
    fun cancellationWinsOverLateUploadAcknowledgement() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            val original = backupAsset(1, BackupState.QUEUED)
            assets.insertDiscovered(original)
            assets.requestCancellation()
            assets.updateTransfer(original.uri, BackupState.UPLOADING, 50, "batch-upload", null, null, 2, "a".repeat(64))
            val retained = assets.pending(true).single()
            assertEquals(BackupState.CANCELLING, retained.state)
            assertEquals("batch-upload", retained.uploadId)
            assertEquals(50L, retained.uploadedBytes)
            assets.recordTransferFailure(original.uri, "Connection lost")
            assertEquals(BackupState.CANCELLING, assets.pending(true).single().state)
            assets.updateTransfer(original.uri, BackupState.COMPLETED, 100, "batch-upload", 1, null, 2, "a".repeat(64))
            assertEquals(0, assets.activeRecordCount())
            assets.recordTransferFailure(original.uri, "Late network error")
            assertEquals(0, assets.activeRecordCount())
        } finally { database.close() }
    }

    @Test
    fun reportsAndRequeuesOnlyUnverifiedCompletedBackups() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(backupAsset(1, BackupState.COMPLETED))
            assets.insertDiscovered(
                backupAsset(2, BackupState.COMPLETED).copy(
                    protocolVersion = 2,
                    contentHash = "a".repeat(64),
                ),
            )
            assets.insertDiscovered(backupAsset(3, BackupState.TERMINAL_FAILED))

            val summary = assets.observeIntegritySummary().first()
            assertEquals(3, summary.totalRecords)
            assertEquals(2, summary.completedRecords)
            assertEquals(1, summary.verifiedRecords)
            assertEquals(1, summary.unverifiedCompletedRecords)
            assertEquals(1, assets.requeueUnverifiedCompleted("repair1"))

            val pending = assets.pending(cameraOnly = false).single()
            assertEquals("media_1_verify_repair1", pending.clientAssetId)
            assertEquals(BackupState.QUEUED, pending.state)
            assertEquals(0, pending.protocolVersion)
            assertNull(pending.contentHash)
        } finally {
            database.close()
        }
    }

    @Test
    fun deletesEveryTerminalBackupRecordWhenIdle() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            for ((index, state) in listOf(
                BackupState.COMPLETED,
                BackupState.TERMINAL_FAILED,
                BackupState.CANCELLED,
            ).withIndex()) {
                assets.insertDiscovered(backupAsset(index, state))
            }

            assertEquals(0, assets.activeRecordCount())
            assertEquals(3, assets.deleteAll())
            assertEquals(emptyList<BackupAssetEntity>(), assets.observeAll().first())
        } finally {
            database.close()
        }
    }

    @Test
    fun reportsActiveBackupRecordsBeforeHistoryCanBeCleared() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(backupAsset(1, BackupState.COMPLETED))
            assets.insertDiscovered(backupAsset(2, BackupState.UPLOADING))

            assertEquals(1, assets.activeRecordCount())
        } finally {
            database.close()
        }
    }

    private fun backupAsset(index: Int, state: BackupState): BackupAssetEntity = BackupAssetEntity(
        uri = "content://media/$index",
        volumeName = "external",
        clientAssetId = "media_$index",
        operationId = "operation-$index",
        displayName = "IMG_$index.jpg",
        mimeType = "image/jpeg",
        byteSize = 100,
        modifiedAt = 1_700_000_000L + index,
        generationModified = index.toLong(),
        folder = "Camera",
        state = state,
        uploadId = if (state == BackupState.COMPLETED) "upload-$index" else null,
        uploadedBytes = if (state == BackupState.COMPLETED) 100 else 0,
        mediaId = if (state == BackupState.COMPLETED) index.toLong() else null,
        errorMessage = null,
        protocolVersion = 0,
        contentHash = null,
    )
}
