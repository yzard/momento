package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import io.github.yzard.momento.core.cache.MediaCacheLimit
import io.github.yzard.momento.core.cache.MediaCacheStore
import java.io.IOException
import kotlinx.coroutines.launch

@Composable
internal fun MediaCacheSettingsSection() {
    val context = LocalContext.current
    val store = remember(context) { MediaCacheStore.get(context) }
    val selected by store.limit.collectAsState()
    val scope = rememberCoroutineScope()
    var open by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    fun choose(limit: MediaCacheLimit) {
        if (saving) return
        saving = true
        scope.launch {
            try { store.setLimit(limit); open = false; error = null }
            catch (_: IOException) { error = "Could not update cache size. Try again." }
            finally { saving = false }
        }
    }
    ListItem(
        headlineContent = { Text("Media cache") },
        supportingContent = { Text("${selected.label} · Cached thumbnails and previews are available offline. Least recently used items are removed first.") },
        leadingContent = { Icon(Icons.Default.Storage, null) },
        modifier = Modifier.clickable { open = true },
    )
    if (open) {
        AlertDialog(
            onDismissRequest = { if (!saving) open = false },
            title = { Text("Cache size") },
            text = {
                Column {
                    MediaCacheLimit.entries.forEach { limit ->
                        ListItem(
                            headlineContent = { Text(limit.label) },
                            leadingContent = { RadioButton(selected == limit, { choose(limit) }, enabled = !saving) },
                            modifier = Modifier.clickable(enabled = !saving) { choose(limit) },
                        )
                    }
                    error?.let { Text(it) }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { open = false }, enabled = !saving) { Text("Cancel") } },
        )
    }
}
