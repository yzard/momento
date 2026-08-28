package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.model.ImportStatus
import io.github.yzard.momento.core.model.JobStatus
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException

@Composable
internal fun ImportAdministration(
    repository: AdministrationRepository,
    webDavUrl: String,
    status: ImportStatus?,
    error: String?,
    refresh: () -> Unit,
) {
    var working by remember { mutableStateOf(false) }
    var actionError by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
    ) {
        item {
            AdminPanel("Local import", "Import media staged on the server or uploaded through WebDAV.") {
                Text("Import directory", style = MaterialTheme.typography.labelLarge)
                Text("/data/imports/", style = MaterialTheme.typography.bodyMedium)
                Text("WebDAV URL", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 12.dp))
                Text(webDavUrl, style = MaterialTheme.typography.bodyMedium)
                Text(importStatusSummary(status), modifier = Modifier.padding(top = 16.dp))
                status?.errors?.forEach { AdminError(it) }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
                Button(
                    onClick = {
                        if (working) return@Button
                        scope.launch {
                            working = true
                            try {
                                repository.localImport()
                                actionError = null
                                refresh()
                            } catch (_: IOException) {
                                actionError = "Could not start local import"
                            } catch (_: HttpException) {
                                actionError = "Could not start local import"
                            } finally {
                                working = false
                            }
                        }
                    },
                    enabled = !working,
                    modifier = Modifier.padding(top = 16.dp),
                ) {
                    Icon(Icons.Default.PlayArrow, null)
                    Text(if (working) "Starting" else "Start local import")
                }
            }
        }
    }
}

@Composable
internal fun MetadataAdministration(
    repository: AdministrationRepository,
    status: JobStatus?,
    error: String?,
    refresh: () -> Unit,
) {
    var busyAction by remember { mutableStateOf<String?>(null) }
    var actionError by remember { mutableStateOf<String?>(null) }
    var pendingAction by remember { mutableStateOf<PendingAdminAction?>(null) }
    val scope = rememberCoroutineScope()

    fun runAction(actionName: String, action: suspend () -> Unit) {
        if (busyAction != null) return
        scope.launch {
            busyAction = actionName
            try {
                action()
                actionError = null
                refresh()
            } catch (_: IOException) {
                actionError = "Could not $actionName metadata"
            } catch (_: HttpException) {
                actionError = "Could not $actionName metadata"
            } finally {
                busyAction = null
            }
        }
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
    ) {
        item {
            AdminPanel("Metadata", "Generate thumbnails and technical metadata before AI processing.") {
                Text(statusSummary(status))
                status?.errors?.forEach { AdminError(it) }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
                Row(
                    modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Button(
                        onClick = { runAction("generate") { repository.generateMetadata() } },
                        enabled = busyAction == null,
                        modifier = Modifier.weight(1f),
                    ) { Text(if (busyAction == "generate") "Generating" else "Generate") }
                    OutlinedButton(
                        onClick = {
                            pendingAction = PendingAdminAction(
                                title = "Reset metadata?",
                                description = "Prepared metadata and related processing work will be reset and regenerated.",
                                confirmLabel = "Reset",
                                execute = { runAction("reset") { repository.resetMetadata() } },
                            )
                        },
                        enabled = busyAction == null,
                        modifier = Modifier.weight(1f),
                    ) { Text("Reset") }
                }
            }
        }
    }
    pendingAction?.let { action ->
        ConfirmationDialog(
            action = action,
            confirm = {
                pendingAction = null
                scope.launch { action.execute() }
            },
            dismiss = { pendingAction = null },
        )
    }
}

