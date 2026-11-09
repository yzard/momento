package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

@Composable
internal fun SettingsControlRow(
    title: String,
    icon: ImageVector?,
    actions: @Composable RowScope.() -> Unit,
) {
    CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onBackground) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (icon != null) Icon(icon, contentDescription = null)
            Text(title, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            Row(verticalAlignment = Alignment.CenterVertically, content = actions)
        }
    }
}

@Composable
internal fun <T> SettingsDropdownRow(
    title: String,
    icon: ImageVector,
    selected: T,
    options: List<T>,
    label: (T) -> String,
    enabled: Boolean,
    onSelect: (T) -> Unit,
    leadingAction: @Composable RowScope.() -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    SettingsControlRow(title = title, icon = icon) {
        leadingAction()
        Box {
            TextButton(onClick = { expanded = true }, enabled = enabled) {
                Text(label(selected))
                Icon(Icons.Default.ArrowDropDown, contentDescription = null)
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                options.forEach { option ->
                    DropdownMenuItem(
                        text = { Text(label(option)) },
                        trailingIcon = { if (selected == option) Icon(Icons.Default.Check, "Selected") },
                        onClick = { expanded = false; onSelect(option) },
                    )
                }
            }
        }
    }
}
