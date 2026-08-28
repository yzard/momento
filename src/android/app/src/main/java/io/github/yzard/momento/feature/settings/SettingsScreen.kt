package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.User

@Composable
fun SettingsScreen(
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    user: User?,
    backupAvailable: Boolean,
    openAdmin: () -> Unit,
    logout: () -> Unit,
) {
    val settings by settingsStore.settings.collectAsState(
        initial = Settings(null, false, true, ThemePreference.SYSTEM),
    )

    MomentoPageScaffold(
        title = "Settings",
        subtitle = null,
        backContentDescription = null,
        onBack = null,
        trailingContent = null,
        reserveBottomControls = true,
        edgeToEdgeContent = false,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
        LazyColumn(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .widthIn(max = 840.dp)
                .fillMaxHeight()
                .fillMaxWidth(),
            contentPadding = contentPadding,
        ) {
            item(key = "account") {
                AccountSettingsSection(
                    repository = repository,
                    user = user,
                    origin = settings.origin,
                    openAdmin = openAdmin,
                    logout = logout,
                )
            }
            item(key = "account-update-divider") { HorizontalDivider() }
            item(key = "update") { AndroidUpdateSettingsSection(repository) }
            item(key = "update-backup-divider") { HorizontalDivider() }
            item(key = "backup") {
                BackupSettingsSection(
                    settings = settings,
                    settingsStore = settingsStore,
                    backupAvailable = backupAvailable,
                )
            }
            item(key = "backup-appearance-divider") { HorizontalDivider() }
            item(key = "appearance") {
                AppearanceSettingsSection(
                    selected = settings.themePreference,
                    settingsStore = settingsStore,
                )
            }
        }
    }
}
