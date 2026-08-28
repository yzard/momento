package io.github.yzard.momento.feature.backup

import android.Manifest
import android.os.Build
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupPermissionsTest {
    @Test fun modernPartialMediaGrantIsNotTreatedAsFullAccess() {
        val partial = backupMediaAccess(
            Build.VERSION_CODES.UPSIDE_DOWN_CAKE,
            setOf(Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED),
        )

        assertEquals(BackupMediaAccess.PARTIAL, partial)
        assertTrue(backupCanReadOriginalMedia(partial, BackupLocationMetadataAccess.PRESERVED))
    }

    @Test fun losslessBackupRequiresLocationMetadataPermission() {
        assertFalse(backupCanReadOriginalMedia(BackupMediaAccess.FULL, BackupLocationMetadataAccess.DENIED))
        assertTrue(backupCanReadOriginalMedia(BackupMediaAccess.FULL, BackupLocationMetadataAccess.PRESERVED))
    }
}
