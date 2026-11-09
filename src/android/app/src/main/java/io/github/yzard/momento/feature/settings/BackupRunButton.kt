package io.github.yzard.momento.feature.settings

import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable

@Composable
internal fun BackupRunButton(
    running: Boolean,
    enabled: Boolean,
    start: () -> Unit,
    cancel: () -> Unit,
) {
    TextButton(onClick = { if (running) cancel() else start() }, enabled = enabled) {
        Text(if (running) "Cancel" else "Back up now")
    }
}
