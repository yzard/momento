package io.github.yzard.momento.feature.settings

import io.github.yzard.momento.core.database.BackupQueueCount
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.BackupState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SettingsScreenTest {
    @Test
    fun summarizesEmptyBackupQueue() {
        assertEquals("No device media has been queued", backupSummary(emptyList()))
    }

    @Test
    fun summarizesBackupStatesInStateOrder() {
        val summary = backupSummary(
            listOf(
                BackupQueueCount(BackupState.COMPLETED, 4),
                BackupQueueCount(BackupState.QUEUED, 2),
            ),
        )

        assertEquals("2 queued · 4 completed", summary)
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
}
