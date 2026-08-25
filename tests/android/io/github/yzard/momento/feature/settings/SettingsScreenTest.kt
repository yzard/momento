package io.github.yzard.momento.feature.settings

import io.github.yzard.momento.core.database.BackupQueueCount
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.BackupState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SettingsScreenTest {
    @Test
    fun summarizesEmptyBackupQueueAsComplete() {
        assertEquals("0/0 media uploaded, all set.", backupSummary(emptyList(), networkAllowed = false))
    }

    @Test
    fun summarizesUploadingBackup() {
        val counts =
            listOf(
                BackupQueueCount(BackupState.SERVER_PROCESSING, 2),
                BackupQueueCount(BackupState.COMPLETED, 1),
                BackupQueueCount(BackupState.QUEUED, 7),
            )

        assertEquals("3/10 media uploaded, uploading...", backupSummary(counts, networkAllowed = true))
        assertEquals("3/10 media uploaded, pausing", backupSummary(counts, networkAllowed = false))
    }

    @Test
    fun summarizesCompletedAndFailedBackups() {
        assertEquals(
            "10/10 media uploaded, all set.",
            backupSummary(listOf(BackupQueueCount(BackupState.COMPLETED, 10)), networkAllowed = false),
        )
        assertEquals(
            "8/10 media uploaded, 2 failed.",
            backupSummary(
                listOf(
                    BackupQueueCount(BackupState.COMPLETED, 8),
                    BackupQueueCount(BackupState.TERMINAL_FAILED, 1),
                    BackupQueueCount(BackupState.CANCELLED, 1),
                ),
                networkAllowed = true,
            ),
        )
    }

    @Test
    fun summarizesPendingServerCancellation() {
        assertEquals(
            "2/3 media uploaded, 1 cancelling...",
            backupSummary(
                listOf(
                    BackupQueueCount(BackupState.COMPLETED, 2),
                    BackupQueueCount(BackupState.CANCELLING, 1),
                ),
                networkAllowed = true,
            ),
        )
    }

    @Test
    fun validatesNewPasswordLengthAndConfirmation() {
        assertEquals("New password must be at least 8 characters", validateNewPassword("short", "short"))
        assertEquals("New passwords do not match", validateNewPassword("longenough", "different"))
        assertNull(validateNewPassword("longenough", "longenough"))
    }

    @Test
    fun labelsThemeChoices() {
        assertEquals("Follow system", themePreferenceLabel(ThemePreference.SYSTEM))
        assertEquals("Light", themePreferenceLabel(ThemePreference.LIGHT))
        assertEquals("Dark", themePreferenceLabel(ThemePreference.DARK))
    }

    @Test
    fun validatesDownloadedUpdateIdentityVersionAndBuildTime() {
        assertEquals(AndroidUpdateDecision.INVALID_PACKAGE, androidUpdateDecision(1, 100, 2, 200, packageMatches = false))
        assertEquals(AndroidUpdateDecision.INVALID_PACKAGE, androidUpdateDecision(1, 100, 2, null, packageMatches = true))
        assertEquals(AndroidUpdateDecision.UP_TO_DATE, androidUpdateDecision(2, 200, 2, 200, packageMatches = true))
        assertEquals(AndroidUpdateDecision.UP_TO_DATE, androidUpdateDecision(2, 200, 2, 100, packageMatches = true))
        assertEquals(AndroidUpdateDecision.UP_TO_DATE, androidUpdateDecision(3, 100, 2, 200, packageMatches = true))
        assertEquals(AndroidUpdateDecision.UPDATE_AVAILABLE, androidUpdateDecision(2, 100, 2, 200, packageMatches = true))
        assertEquals(AndroidUpdateDecision.UPDATE_AVAILABLE, androidUpdateDecision(2, 200, 3, 100, packageMatches = true))
    }

    @Test
    fun parsesPackagedBuildTime() {
        assertEquals(1_725_000_123_456L, androidBuildTimeMillis("epochMillis:1725000123456"))
        assertNull(androidBuildTimeMillis("1725000123456"))
        assertNull(androidBuildTimeMillis("epochMillis:0"))
        assertNull(androidBuildTimeMillis(null))
    }

    @Test
    fun updateCacheNamesCarryTheCandidateVersionAndBuildTime() {
        val identity = AndroidUpdateIdentity(1_002_003, 1_725_000_123_456)
        assertEquals("momento-update-1002003-1725000123456.apk", androidUpdateCacheFilename(identity))
        assertEquals(identity, cachedAndroidUpdateIdentity("momento-update-1002003-1725000123456.apk"))
        assertNull(cachedAndroidUpdateIdentity("momento-update-1002003.apk"))
        assertNull(cachedAndroidUpdateIdentity("momento-update.download"))
    }
}
