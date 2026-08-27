package io.github.yzard.momento.feature.backup

import android.Manifest
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupCapabilities
import io.github.yzard.momento.core.model.BackupUploadResponse
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupTest {
    @Test fun serverCapabilityPreventsAnExistingWorkerFromUploading() {
        val enabled = BackupCapabilities(true, 2, 100, 100, 1, 24)
        val disabled = enabled.copy(enabled = false)
        assertTrue(backupCanRun(enabled))
        assertFalse(backupCanRun(disabled))
        assertFalse(backupCanRun(enabled.copy(protocolVersion = 1)))
    }
    @Test fun discoveredAssetsStartQueuedWithStableClientId() {
        val asset = discoveredAsset(
            "content://media/42",
            42,
            "external",
            null,
            "IMG_0042.jpg",
            "image/jpeg",
            12,
            100,
            0,
            "Camera",
        )
        assertEquals("media_42", asset.clientAssetId)
        assertEquals(BackupState.QUEUED, asset.state)
        assertTrue(asset.operationId.isNotBlank())
    }

    @Test fun backupResetGenerationProducesANewServerClientAssetId() {
        assertEquals("media_42", backupClientAssetId(42, "external", null))
        assertEquals("media_reset123_42", backupClientAssetId(42, "external", "reset123"))
        assertEquals("media_external_primary_42", backupClientAssetId(42, "external_primary", null))
        assertFalse(
            backupClientAssetId(42, "external", null) ==
                backupClientAssetId(42, "external", "reset123"),
        )
    }

    @Test fun retriesOnlyTransientHttpResponses() {
        assertTrue(isRetryable(408))
        assertTrue(isRetryable(429))
        assertTrue(isRetryable(503))
        assertFalse(isRetryable(400))
        assertFalse(isRetryable(401))
    }

    @Test fun serverProcessingIsRetriedWithoutBlockingTheWorker() {
        assertEquals(BackupProgress.WAITING_FOR_SERVER, serverProgress(BackupUploadResponse("upload", "processing", 10, 10, "hash", null, null)))
    }

    @Test fun completedAndFailedServerResponsesAreTerminal() {
        assertEquals(BackupProgress.COMPLETED, serverProgress(BackupUploadResponse("upload", "completed", 10, 10, "hash", 4, null)))
        assertEquals(BackupProgress.COMPLETED, serverProgress(BackupUploadResponse("upload", "failed", 10, 10, "hash", null, "bad file")))
    }

    @Test fun calculatesSha256ForOnlyTheUploadedChunkBytes() {
        val buffer = "hello-extra".toByteArray()
        assertEquals(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            sha256Hex(buffer, 5),
        )
    }

    @Test fun cancellationKeepsWritableAndTransientSessionsRetryable() {
        assertEquals(BackupState.CANCELLING, cancellationState("uploading"))
        assertEquals(BackupState.SERVER_PROCESSING, cancellationState("processing"))
        assertTrue(isCancellationRetryable(409))
        assertTrue(isCancellationRetryable(503))
        assertFalse(isCancellationRetryable(400))
    }

    @Test fun cancellationPreservesServerTerminalStates() {
        assertEquals(BackupState.CANCELLED, cancellationState("cancelled"))
        assertEquals(BackupState.COMPLETED, cancellationState("completed"))
        assertEquals(BackupState.TERMINAL_FAILED, cancellationState("failed"))
    }

    @Test fun backupNetworkRequiresValidatedInternet() {
        assertFalse(backupNetworkAllowed(allowMobileData = true, hasValidatedInternet = false, unmetered = true))
        assertFalse(backupNetworkAllowed(allowMobileData = false, hasValidatedInternet = false, unmetered = true))
    }

    @Test fun backupNetworkRespectsMobileDataPreference() {
        assertTrue(backupNetworkAllowed(allowMobileData = true, hasValidatedInternet = true, unmetered = false))
        assertFalse(backupNetworkAllowed(allowMobileData = false, hasValidatedInternet = true, unmetered = false))
        assertTrue(backupNetworkAllowed(allowMobileData = false, hasValidatedInternet = true, unmetered = true))
    }

    @Test fun immediateAndDailyBackupsUseDifferentWorkNames() {
        assertEquals("momento_backup_now", IMMEDIATE_BACKUP_WORK_NAME)
        assertEquals("momento_backup_periodic", PERIODIC_BACKUP_WORK_NAME)
        assertEquals("momento_backup", LEGACY_BACKUP_WORK_NAME)
        assertFalse(IMMEDIATE_BACKUP_WORK_NAME == PERIODIC_BACKUP_WORK_NAME)
    }

    @Test fun requestsAndroid14SelectedMediaPermissionAlongsideMediaTypes() {
        assertArrayEquals(
            arrayOf(
                Manifest.permission.READ_MEDIA_IMAGES,
                Manifest.permission.READ_MEDIA_VIDEO,
                Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
            ),
            backupReadPermissions(34),
        )
        assertArrayEquals(
            arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO),
            backupReadPermissions(33),
        )
        assertArrayEquals(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE), backupReadPermissions(32))
    }

    @Test fun requestsPhotoLocationPermissionOnAndroid10AndNewer() {
        assertArrayEquals(
            arrayOf(
                Manifest.permission.READ_MEDIA_IMAGES,
                Manifest.permission.READ_MEDIA_VIDEO,
                Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
                Manifest.permission.ACCESS_MEDIA_LOCATION,
            ),
            backupPermissions(34),
        )
        assertArrayEquals(
            arrayOf(
                Manifest.permission.READ_MEDIA_IMAGES,
                Manifest.permission.READ_MEDIA_VIDEO,
                Manifest.permission.ACCESS_MEDIA_LOCATION,
            ),
            backupPermissions(33),
        )
        assertArrayEquals(
            arrayOf(
                Manifest.permission.READ_EXTERNAL_STORAGE,
                Manifest.permission.ACCESS_MEDIA_LOCATION,
            ),
            backupPermissions(32),
        )
        assertArrayEquals(
            arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE),
            backupPermissions(28),
        )
    }

    @Test fun distinguishesFullPartialAndDeniedMediaAccess() {
        assertEquals(
            BackupMediaAccess.FULL,
            backupMediaAccess(
                34,
                setOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO),
            ),
        )
        assertEquals(
            BackupMediaAccess.PARTIAL,
            backupMediaAccess(34, setOf(Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED)),
        )
        assertEquals(
            BackupMediaAccess.PARTIAL,
            backupMediaAccess(33, setOf(Manifest.permission.READ_MEDIA_IMAGES)),
        )
        assertEquals(BackupMediaAccess.DENIED, backupMediaAccess(34, emptySet()))
        assertEquals(
            BackupMediaAccess.FULL,
            backupMediaAccess(32, setOf(Manifest.permission.READ_EXTERNAL_STORAGE)),
        )
        assertEquals(BackupMediaAccess.DENIED, backupMediaAccess(32, emptySet()))
    }

    @Test fun requiresPhotoLocationAccessOnlyOnAndroid10AndNewer() {
        assertEquals(
            BackupLocationMetadataAccess.PRESERVED,
            backupLocationMetadataAccess(28, emptySet()),
        )
        assertEquals(
            BackupLocationMetadataAccess.DENIED,
            backupLocationMetadataAccess(29, emptySet()),
        )
        assertEquals(
            BackupLocationMetadataAccess.PRESERVED,
            backupLocationMetadataAccess(29, setOf(Manifest.permission.ACCESS_MEDIA_LOCATION)),
        )
    }

    @Test fun backupRunsOnlyWhenOriginalMediaCanBeRead() {
        assertTrue(
            backupCanReadOriginalMedia(
                BackupMediaAccess.FULL,
                BackupLocationMetadataAccess.PRESERVED,
            ),
        )
        assertTrue(
            backupCanReadOriginalMedia(
                BackupMediaAccess.PARTIAL,
                BackupLocationMetadataAccess.PRESERVED,
            ),
        )
        assertFalse(
            backupCanReadOriginalMedia(
                BackupMediaAccess.DENIED,
                BackupLocationMetadataAccess.PRESERVED,
            ),
        )
        assertFalse(
            backupCanReadOriginalMedia(
                BackupMediaAccess.FULL,
                BackupLocationMetadataAccess.DENIED,
            ),
        )
    }

    @Test fun usesOriginalMediaUriWhenAndroidCanRedactLocationMetadata() {
        assertFalse(
            backupUsesOriginalMediaUri(28, BackupLocationMetadataAccess.PRESERVED),
        )
        assertTrue(
            backupUsesOriginalMediaUri(29, BackupLocationMetadataAccess.PRESERVED),
        )
        assertFalse(
            backupUsesOriginalMediaUri(29, BackupLocationMetadataAccess.DENIED),
        )
    }

    @Test fun recognizesCommonCameraDirectories() {
        assertTrue(isCameraMediaFolder("Camera", "DCIM/Camera/"))
        assertTrue(isCameraMediaFolder("100ANDRO", "DCIM/100ANDRO/"))
        assertTrue(isCameraMediaFolder("Camera Roll", "Pictures/Camera/"))
        assertTrue(isCameraMediaFolder("Camera Roll", "Pictures/Camera Roll/"))
        assertTrue(isCameraMediaFolder("Camera", null))
    }

    @Test fun excludesScreenshotAndRecordingDirectoriesFromCameraBackup() {
        assertFalse(isCameraMediaFolder("Screenshots", "DCIM/Screenshots/"))
        assertFalse(isCameraMediaFolder("Screen recordings", "DCIM/Screen recordings/"))
        assertFalse(isCameraMediaFolder("ScreenRecorder", "DCIM/ScreenRecorder/"))
        assertFalse(isCameraMediaFolder("Screenshots", "Pictures/Screenshots/"))
    }

    @Test fun describesBackupMediaAccess() {
        assertEquals("All photos and videos are available for backup", backupMediaAccessLabel(BackupMediaAccess.FULL))
        assertEquals(
            "Only selected photos or media types are available for backup",
            backupMediaAccessLabel(BackupMediaAccess.PARTIAL),
        )
        assertEquals(
            "Photo and video access is required before backup can run",
            backupMediaAccessLabel(BackupMediaAccess.DENIED),
        )
        assertEquals(
            "Photo location metadata will be preserved",
            backupLocationMetadataAccessLabel(BackupLocationMetadataAccess.PRESERVED),
        )
        assertEquals(
            "Photo location access is required for lossless backup",
            backupLocationMetadataAccessLabel(BackupLocationMetadataAccess.DENIED),
        )
    }
}
