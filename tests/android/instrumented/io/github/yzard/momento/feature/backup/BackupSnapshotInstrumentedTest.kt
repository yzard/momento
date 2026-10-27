package io.github.yzard.momento.feature.backup

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class BackupSnapshotInstrumentedTest {
    @Test fun replacementKeepsExactSnapshotMetadataForCreateRetries() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val oldId = UUID.randomUUID().toString()
        val newId = UUID.randomUUID().toString()
        val directory = File(context.noBackupFilesDir, "backup-snapshots").apply { mkdirs() }
        val oldFile = File(directory, "$oldId.original")
        val oldMetadata = File(directory, "$oldId.metadata.json")
        try {
            oldFile.writeText("original bytes")
            oldMetadata.writeText("{\"capturedAt\":\"unchanged\"}")
            val snapshot = BackupSnapshot(oldFile, oldFile.length(), "hash", JsonObject(emptyMap()))
            val replacement = rekeyBackupSnapshot(context, snapshot, oldId, newId)
            assertEquals("original bytes", replacement.file.readText())
            assertEquals("{\"capturedAt\":\"unchanged\"}", File(directory, "$newId.metadata.json").readText())
            assertEquals(snapshot.contentHash, replacement.contentHash)
            assertFalse(oldFile.exists())
            assertFalse(oldMetadata.exists())
            val restored = rekeyBackupSnapshot(context, replacement, newId, oldId)
            assertEquals("original bytes", restored.file.readText())
        } finally {
            deleteBackupSnapshot(context, oldId)
            deleteBackupSnapshot(context, newId)
        }
    }
}
