package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

@Composable
internal fun BackupProgressContent(status: String, running: Boolean, completed: Long, total: Long, errors: List<String>) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("backupProgressContent"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(status, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.fillMaxWidth().testTag("backupStatus"))
        if (running) {
            Row(Modifier.fillMaxWidth().testTag("backupMediaRow"), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                LinearProgressIndicator(progress = { if (total > 0) (completed.toFloat() / total).coerceIn(0f, 1f) else 0f },
                    modifier = Modifier.weight(1f).testTag("backupMediaProgress"))
                Text("$completed/$total", style = MaterialTheme.typography.labelSmall, modifier = Modifier.testTag("backupMediaCount"))
            }
        }
        errors.forEach { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.fillMaxWidth().testTag("backupUploadError")) }
    }
}
