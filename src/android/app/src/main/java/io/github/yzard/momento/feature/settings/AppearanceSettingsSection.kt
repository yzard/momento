package io.github.yzard.momento.feature.settings

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Palette
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
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
    val scope = rememberCoroutineScope()
    SettingsDropdownRow(
        title = "Appearance",
        icon = Icons.Default.Palette,
        selected = selected,
        options = ThemePreference.entries,
        label = ::themePreferenceLabel,
        enabled = true,
        leadingAction = {},
        onSelect = { choice -> scope.launch { settingsStore.setThemePreference(choice) } },
    )
}
