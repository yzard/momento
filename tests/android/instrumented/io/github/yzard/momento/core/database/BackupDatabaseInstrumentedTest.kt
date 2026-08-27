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
    fun cancelledAssetIsRequeuedWhenTheNextScanFindsIt() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val database = Room.inMemoryDatabaseBuilder(context, BackupDatabase::class.java).build()
        try {
            val assets = database.backupAssetDao()
            assets.insertDiscovered(
                BackupAssetEntity(
                    uri = "content://media/42",
                    clientAssetId = "media_42",
                    operationId = "cancelled-operation",
                    displayName = "IMG_0042.jpg",
                    mimeType = "image/jpeg",
                    byteSize = 100,
                    modifiedAt = 1_700_000_000,
                    folder = "Camera",
                    state = BackupState.CANCELLED,
                    uploadId = "upload-42",
                    uploadedBytes = 50,
                    mediaId = null,
                    errorMessage = "Cancelled by user",
                ),
            )

            assets.reconcileDiscovered(
                uri = "content://media/42",
                clientAssetId = "media_42",
                operationId = "daily-operation",
                displayName = "IMG_0042.jpg",
                mimeType = "image/jpeg",
                byteSize = 100,
                modifiedAt = 1_700_000_000,
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
        clientAssetId = "media_$index",
        operationId = "operation-$index",
        displayName = "IMG_$index.jpg",
        mimeType = "image/jpeg",
        byteSize = 100,
        modifiedAt = 1_700_000_000L + index,
        folder = "Camera",
        state = state,
        uploadId = if (state == BackupState.COMPLETED) "upload-$index" else null,
        uploadedBytes = if (state == BackupState.COMPLETED) 100 else 0,
        mediaId = if (state == BackupState.COMPLETED) index.toLong() else null,
        errorMessage = null,
    )
}
