package io.github.yzard.momento.feature.settings

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
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
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    fun choose(limit: MediaCacheLimit) {
        if (saving) return
        saving = true
        scope.launch {
            try { store.setLimit(limit); error = null }
            catch (_: IOException) { error = "Could not update cache size. Try again." }
            finally { saving = false }
        }
    }
    fun clear() {
        if (saving) return
        saving = true
        scope.launch {
            try { store.clear(); error = null }
            catch (_: IOException) { error = "Could not clear local cache. Try again." }
            finally { saving = false }
        }
    }
    SettingsDropdownRow(
        title = "Local Cache Limit",
        icon = Icons.Default.Storage,
        selected = selected,
        options = MediaCacheLimit.entries,
        label = { it.label },
        enabled = !saving,
        onSelect = ::choose,
        leadingAction = {
            TextButton(onClick = ::clear, enabled = !saving,
                colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Clear") }
        },
    )
    error?.let { message ->
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text("Local Cache Limit") },
            text = { Text(message) },
            confirmButton = { TextButton(onClick = { error = null }) { Text("OK") } },
        )
    }
}
