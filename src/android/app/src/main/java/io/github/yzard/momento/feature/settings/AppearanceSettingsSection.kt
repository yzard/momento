package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import kotlinx.coroutines.launch

fun themePreferenceLabel(themePreference: ThemePreference): String = when (themePreference) {
    ThemePreference.SYSTEM -> "Follow system"
    ThemePreference.LIGHT -> "Light"
    ThemePreference.DARK -> "Dark"
}

@Composable
internal fun AppearanceSettingsSection(
    selected: ThemePreference,
    settingsStore: SettingsStore,
) {
    var dialogOpen by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    ListItem(
        headlineContent = { Text("Appearance") },
        supportingContent = { Text(themePreferenceLabel(selected)) },
        leadingContent = { Icon(Icons.Default.Palette, null) },
        modifier = Modifier.clickable { dialogOpen = true },
    )

    if (dialogOpen) {
        AlertDialog(
            onDismissRequest = { dialogOpen = false },
            title = { Text("Appearance") },
            text = {
                Column {
                    ThemePreference.entries.forEach { themePreference ->
                        ListItem(
                            headlineContent = { Text(themePreferenceLabel(themePreference)) },
                            leadingContent = {
                                RadioButton(
                                    selected = themePreference == selected,
                                    onClick = {
                                        scope.launch { settingsStore.setThemePreference(themePreference) }
                                        dialogOpen = false
                                    },
                                )
                            },
                            modifier = Modifier.clickable {
                                scope.launch { settingsStore.setThemePreference(themePreference) }
                                dialogOpen = false
                            },
                        )
                    }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { dialogOpen = false }) { Text("Cancel") } },
        )
    }
}
