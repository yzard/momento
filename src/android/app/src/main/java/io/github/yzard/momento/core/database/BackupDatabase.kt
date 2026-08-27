package io.github.yzard.momento.core.database

import android.content.Context
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.ColumnInfo
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.TypeConverter
import androidx.room.TypeConverters
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import io.github.yzard.momento.core.model.BackupState
import kotlinx.coroutines.flow.Flow

class BackupConverters {
    @TypeConverter fun stateToString(value: BackupState): String = value.name
    @TypeConverter fun stringToState(value: String): BackupState = BackupState.valueOf(value)
}

@Entity(tableName = "backup_assets")
data class BackupAssetEntity(
    @androidx.room.PrimaryKey val uri: String,
    @ColumnInfo(defaultValue = "'external'") val volumeName: String,
    val clientAssetId: String,
    val operationId: String,
    val displayName: String,
    val mimeType: String,
    val byteSize: Long,
    val modifiedAt: Long,
    @ColumnInfo(defaultValue = "0") val generationModified: Long,
    val folder: String,
    val state: BackupState,
    val uploadId: String?,
    val uploadedBytes: Long,
    val mediaId: Long?,
    val errorMessage: String?,
    @ColumnInfo(defaultValue = "0") val protocolVersion: Int,
    val contentHash: String?,
)

data class BackupQueueCount(val state: BackupState, val count: Long)

data class BackupIntegritySummary(
    val totalRecords: Long,
    val completedRecords: Long,
    val verifiedRecords: Long,
    val unverifiedCompletedRecords: Long,
)

@Dao
interface BackupAssetDao {
    @Query("SELECT * FROM backup_assets WHERE state IN ('QUEUED', 'FAILED', 'UPLOADING', 'COMPLETING', 'SERVER_PROCESSING', 'CANCELLING') AND (:cameraOnly = 0 OR folder = 'Camera') ORDER BY modifiedAt")
    suspend fun pending(cameraOnly: Boolean): List<BackupAssetEntity>

    @Query("SELECT * FROM backup_assets WHERE state IN ('CANCELLING', 'SERVER_PROCESSING') ORDER BY modifiedAt")
    suspend fun cancellationPending(): List<BackupAssetEntity>

    @Query("UPDATE backup_assets SET state = 'CANCELLING', errorMessage = NULL WHERE state IN ('QUEUED', 'FAILED', 'UPLOADING', 'COMPLETING', 'SERVER_PROCESSING')")
    suspend fun requestCancellation(): Int

    @Query("SELECT COUNT(*) FROM backup_assets WHERE state IN ('QUEUED', 'FAILED', 'UPLOADING', 'COMPLETING', 'SERVER_PROCESSING', 'CANCELLING')")
    suspend fun activeRecordCount(): Int

    @Query("DELETE FROM backup_assets")
    suspend fun deleteAll(): Int

    @Query("SELECT * FROM backup_assets ORDER BY modifiedAt DESC") fun observeAll(): Flow<List<BackupAssetEntity>>
    @Query("SELECT errorMessage FROM backup_assets WHERE errorMessage IS NOT NULL AND errorMessage != '' ORDER BY modifiedAt DESC LIMIT 1")
    fun observeLatestError(): Flow<String?>
    @Query("SELECT state, COUNT(*) AS count FROM backup_assets WHERE :cameraOnly = 0 OR folder = 'Camera' GROUP BY state")
    fun observeCounts(cameraOnly: Boolean): Flow<List<BackupQueueCount>>
    @Query("""
        SELECT COUNT(*) AS totalRecords,
               COALESCE(SUM(CASE WHEN state = 'COMPLETED' THEN 1 ELSE 0 END), 0) AS completedRecords,
               COALESCE(SUM(CASE WHEN state = 'COMPLETED' AND protocolVersion = 2 AND length(contentHash) = 64 THEN 1 ELSE 0 END), 0) AS verifiedRecords,
               COALESCE(SUM(CASE WHEN state = 'COMPLETED' AND (protocolVersion != 2 OR contentHash IS NULL OR length(contentHash) != 64) THEN 1 ELSE 0 END), 0) AS unverifiedCompletedRecords
          FROM backup_assets
    """)
    fun observeIntegritySummary(): Flow<BackupIntegritySummary>
    @Insert(onConflict = OnConflictStrategy.IGNORE) suspend fun insertDiscovered(asset: BackupAssetEntity): Long

    @Query("""
        UPDATE backup_assets SET
            volumeName = :volumeName,
            clientAssetId = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN :clientAssetId ELSE clientAssetId END,
            operationId = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN :operationId ELSE operationId END,
            displayName = :displayName,
            mimeType = :mimeType,
            byteSize = :byteSize,
            modifiedAt = :modifiedAt,
            generationModified = :generationModified,
            folder = :folder,
            state = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN 'QUEUED' ELSE state END,
            uploadId = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN NULL ELSE uploadId END,
            uploadedBytes = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN 0 ELSE uploadedBytes END,
            mediaId = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN NULL ELSE mediaId END,
            errorMessage = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN NULL ELSE errorMessage END,
            protocolVersion = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN 0 ELSE protocolVersion END,
            contentHash = CASE WHEN byteSize != :byteSize OR modifiedAt != :modifiedAt OR generationModified != :generationModified OR state = 'CANCELLED' THEN NULL ELSE contentHash END
        WHERE uri = :uri
    """)
    suspend fun reconcileDiscovered(uri: String, volumeName: String, clientAssetId: String, operationId: String, displayName: String, mimeType: String, byteSize: Long, modifiedAt: Long, generationModified: Long, folder: String)

    @Query("UPDATE backup_assets SET state = :state, uploadedBytes = :uploadedBytes, uploadId = :uploadId, mediaId = :mediaId, errorMessage = :errorMessage, protocolVersion = :protocolVersion, contentHash = :contentHash WHERE uri = :uri")
    suspend fun updateTransfer(uri: String, state: BackupState, uploadedBytes: Long, uploadId: String?, mediaId: Long?, errorMessage: String?, protocolVersion: Int, contentHash: String?)

    @Query("""
        UPDATE backup_assets SET
            clientAssetId = clientAssetId || '_verify_' || :generation,
            operationId = lower(hex(randomblob(16))),
            state = 'QUEUED',
            uploadId = NULL,
            uploadedBytes = 0,
            mediaId = NULL,
            errorMessage = NULL,
            protocolVersion = 0,
            contentHash = NULL
        WHERE state = 'COMPLETED'
          AND (protocolVersion != 2 OR contentHash IS NULL OR length(contentHash) != 64)
    """)
    suspend fun requeueUnverifiedCompleted(generation: String): Int
}

@Database(entities = [BackupAssetEntity::class], version = 5, exportSchema = true)
@TypeConverters(BackupConverters::class)
abstract class BackupDatabase : RoomDatabase() {
    abstract fun backupAssetDao(): BackupAssetDao

    companion object {
        val MIGRATION_3_4 = object : Migration(3, 4) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE backup_assets ADD COLUMN volumeName TEXT NOT NULL DEFAULT 'external'")
                db.execSQL("ALTER TABLE backup_assets ADD COLUMN generationModified INTEGER NOT NULL DEFAULT 0")
            }
        }

        val MIGRATION_4_5 = object : Migration(4, 5) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE backup_assets ADD COLUMN protocolVersion INTEGER NOT NULL DEFAULT 0")
                db.execSQL("ALTER TABLE backup_assets ADD COLUMN contentHash TEXT")
            }
        }

        fun create(context: Context): BackupDatabase = Room.databaseBuilder(context, BackupDatabase::class.java, "momento-backup.db")
            .addMigrations(MIGRATION_3_4, MIGRATION_4_5)
            .build()
    }
}
