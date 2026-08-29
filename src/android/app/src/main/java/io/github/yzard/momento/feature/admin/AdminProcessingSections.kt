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
import androidx.compose.material3.LinearProgressIndicator
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
internal fun ImportAdministrationScreen(
    repository: AdministrationRepository,
    webDAVURL: String,
) {
    val pollingState = rememberAdminPollingState(
        repositoryKey = repository,
        failureMessage = "Could not load import status",
        load = repository::importStatus,
    )
    AdminPageScaffold(
        section = AdminSection.IMPORT,
        refreshing = pollingState.refreshing,
        refresh = pollingState.refresh,
    ) {
        ImportAdministration(
            repository = repository,
            webDAVURL = webDAVURL,
            status = pollingState.value,
            error = pollingState.error,
            refresh = pollingState.refresh,
        )
    }
}

@Composable
internal fun ImportAdministration(
    repository: AdministrationRepository,
    webDAVURL: String,
    status: ImportStatus?,
    error: String?,
    refresh: () -> Unit,
) {
    var working by remember { mutableStateOf(false) }
    var actionError by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val progress = status?.takeIf { it.totalFiles > 0 }?.let {
        it.processedFiles.toFloat() / it.totalFiles.toFloat()
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            AdminPanel("Local Import", "Import media staged in the server import directory.") {
                Text("Import directory", style = MaterialTheme.typography.labelLarge)
                Text("/data/imports/", style = MaterialTheme.typography.bodyMedium)
                AdminStatusMetrics(
                    listOf(
                        AdminMetric("Status", status?.status ?: "—", false),
                        AdminMetric("Imported", status?.successfulImports?.toString() ?: "—", false),
                        AdminMetric(
                            "Failed",
                            status?.failedImports?.toString() ?: "—",
                            (status?.failedImports ?: 0) > 0,
                        ),
                        AdminMetric("Total Media", status?.totalMedia?.toString() ?: "—", false),
                    ),
                )
                if (status?.status == "running" && progress != null) {
                    Text(
                        "${status.processedFiles} / ${status.totalFiles} files",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    LinearProgressIndicator(
                        progress = { progress.coerceIn(0f, 1f) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
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
                                actionError = "Could not start import"
                            } catch (_: HttpException) {
                                actionError = "Could not start import"
                            } finally {
                                working = false
                            }
                        }
                    },
                    enabled = !working && status?.status != "running",
                    modifier = Modifier.padding(top = 8.dp),
                ) {
                    Icon(Icons.Default.PlayArrow, null)
                    Text(if (working) "Starting" else "Start import")
                }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
                AdminFailureLog("Import failure log", status?.errors.orEmpty())
            }
        }
        item {
            AdminPanel(
                "WebDAV",
                "Upload media through a WebDAV client using your Momento credentials.",
            ) {
                Text("WebDAV URL", style = MaterialTheme.typography.labelLarge)
                SelectionText(webDAVURL)
                Text(
                    "Use the same username and password used to sign in to Momento.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun SelectionText(value: String) {
    androidx.compose.foundation.text.selection.SelectionContainer {
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
internal fun MetadataAdministrationScreen(repository: AdministrationRepository) {
    val pollingState = rememberAdminPollingState(
        repositoryKey = repository,
        failureMessage = "Could not load metadata status",
        load = repository::metadataStatus,
    )
    AdminPageScaffold(
        section = AdminSection.METADATA,
        refreshing = pollingState.refreshing,
        refresh = pollingState.refresh,
    ) {
        MetadataAdministration(
            repository = repository,
            status = pollingState.value,
            error = pollingState.error,
            refresh = pollingState.refresh,
        )
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
                AdminStatusMetrics(
                    listOf(
                        AdminMetric("Queued", status?.queuedJobs?.toString() ?: "—", false),
                        AdminMetric("Processing", status?.processingJobs?.toString() ?: "—", false),
                        AdminMetric("Completed", status?.completedJobs?.toString() ?: "—", false),
                        AdminMetric(
                            "Failed",
                            status?.failedJobs?.toString() ?: "—",
                            (status?.failedJobs ?: 0) > 0,
                        ),
                    ),
                )
                Row(
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
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
                                title = "Reset metadata and AI data?",
                                description = "This removes generated metadata and related AI data, then queues metadata generation again. Existing original media is preserved.",
                                confirmLabel = "Reset & regenerate",
                                execute = { runAction("reset") { repository.resetMetadata() } },
                            )
                        },
                        enabled = busyAction == null,
                        modifier = Modifier.weight(1f),
                    ) { Text("Reset & regenerate") }
                }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
                AdminFailureLog("Metadata failure log", status?.errors.orEmpty())
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
