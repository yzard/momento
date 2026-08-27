package io.github.yzard.momento.core.database

import androidx.room.Room
import androidx.room.testing.MigrationTestHelper
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yzard.momento.core.model.BackupState
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.Rule
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BackupDatabaseInstrumentedTest {
    @get:Rule
    val migrationHelper = MigrationTestHelper(
        InstrumentationRegistry.getInstrumentation(),
        BackupDatabase::class.java,
    )

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
    fun migratesVersionThreeAssetsWithExplicitGenerationDefaults() {
        val databaseName = "backup-migration-${System.nanoTime()}"
        migrationHelper.createDatabase(databaseName, 3).apply {
            execSQL(
                """
                INSERT INTO backup_assets (
                    uri, clientAssetId, operationId, displayName, mimeType, byteSize,
                    modifiedAt, folder, state, uploadId, uploadedBytes, mediaId, errorMessage
                ) VALUES (
                    'content://media/7', 'media_7', 'operation-7', 'IMG_7.jpg',
                    'image/jpeg', 100, 1700000000, 'Camera', 'COMPLETED',
                    'upload-7', 100, 7, NULL
                )
                """.trimIndent(),
            )
            close()
        }

        migrationHelper.runMigrationsAndValidate(
            databaseName,
            4,
            true,
            BackupDatabase.MIGRATION_3_4,
        ).use { database ->
            database.query(
                "SELECT volumeName, generationModified FROM backup_assets WHERE uri = 'content://media/7'",
            ).use { cursor ->
                cursor.moveToFirst()
                assertEquals("external", cursor.getString(0))
                assertEquals(0, cursor.getLong(1))
            }
        }
    }

    @Test
    fun migratesVersionFourAssetsAsUnverifiedHistory() {
        val databaseName = "backup-migration-${System.nanoTime()}"
        migrationHelper.createDatabase(databaseName, 4).apply {
            execSQL(
                """
                INSERT INTO backup_assets (
                    uri, volumeName, clientAssetId, operationId, displayName, mimeType,
                    byteSize, modifiedAt, generationModified, folder, state, uploadId,
                    uploadedBytes, mediaId, errorMessage
                ) VALUES (
                    'content://media/9', 'external', 'media_9', 'operation-9', 'IMG_9.jpg',
                    'image/jpeg', 100, 1700000000, 4, 'Camera', 'COMPLETED',
                    'upload-9', 100, 9, NULL
                )
                """.trimIndent(),
            )
            close()
        }

        migrationHelper.runMigrationsAndValidate(
            databaseName,
            5,
            true,
            BackupDatabase.MIGRATION_4_5,
        ).use { database ->
            database.query(
                "SELECT protocolVersion, contentHash FROM backup_assets WHERE uri = 'content://media/9'",
            ).use { cursor ->
                cursor.moveToFirst()
                assertEquals(0, cursor.getInt(0))
                assertNull(cursor.getString(1))
            }
        }
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
