package io.github.yzard.momento.feature.settings

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable

@Composable
internal fun BackupDiagnosticsRow(busy: Boolean, copy: () -> Unit, share: () -> Unit) {
    SettingsControlRow(title = "Backup diagnostics", icon = null) {
        TextButton(onClick = copy, enabled = !busy) { Text("Copy") }
        IconButton(onClick = share, enabled = !busy) { Icon(Icons.Default.Share, "Share logs") }
    }
}

@Composable
internal fun BackupHistoryRow(
    canRepair: Boolean,
    canClear: Boolean,
    repairBusy: Boolean,
    clearBusy: Boolean,
    repair: () -> Unit,
    clear: () -> Unit,
) {
    SettingsControlRow(title = "Backup history", icon = null) {
        TextButton(onClick = repair, enabled = canRepair && !repairBusy && !clearBusy) {
            Text(if (repairBusy) "Reverifying…" else "Reverify")
        }
        TextButton(
            onClick = clear,
            enabled = canClear && !clearBusy,
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
        ) { Text(if (clearBusy) "Clearing…" else "Clear") }
    }
}
