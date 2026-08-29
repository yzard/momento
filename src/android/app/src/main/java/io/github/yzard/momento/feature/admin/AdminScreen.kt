package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.data.RequestResult
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.data.runRequest
import io.github.yzard.momento.core.data.userMessage
import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiStatusResponse
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

enum class AdminSection(val label: String) {
    IMPORT("Import"),
    METADATA("Metadata"),
    AI("AI"),
    USERS("User Management"),
}

enum class AdminAiFeature(val identifier: String, val label: String) {
    OCR("ocr", "OCR"),
    IMAGE_TAGGING("image_tagging", "Image tagging"),
    SCREENSHOT_DETECTION("screenshot_detection", "Screenshot detection"),
    DOCUMENT_DETECTION("document_detection", "Document detection"),
    IMAGE_AESTHETICS("image_aesthetics", "Image aesthetics"),
    DEDUPLICATE("deduplicate", "Deduplication"),
    FACE_DETECTION("face_detection", "Face detection"),
}

internal val cronFieldLabels = listOf("Minute", "Hour", "Day", "Month", "Weekday")

fun splitCronExpression(cronExpression: String): List<String> {
    val fields = cronExpression.trim().split(Regex("\\s+"))
    if (fields.size != cronFieldLabels.size) return List(cronFieldLabels.size) { "" }
    return fields
}

fun joinCronFields(cronFields: List<String>): String {
    require(cronFields.size == cronFieldLabels.size) { "A cron schedule must contain five fields" }
    return cronFields.joinToString(" ") { it.trim() }
}

fun validCronFields(cronFields: List<String>): Boolean {
    if (cronFields.size != cronFieldLabels.size) return false
    return cronFields.all { field -> field.trim().isNotEmpty() && !field.trim().contains(Regex("\\s")) }
}

fun toggledRole(role: String): String = if (role == "admin") "user" else "admin"

fun isActiveAiState(state: String?): Boolean =
    state in setOf("queued", "submitting", "submitted", "running", "cancelling")

internal fun aiJobCounts(status: AiStatusResponse?, feature: AdminAiFeature): AiJobCounts? =
    if (feature == AdminAiFeature.DEDUPLICATE) {
        status?.deduplicate?.jobs
    } else {
        status?.tasks?.firstOrNull { it.task == feature.identifier }?.jobs
    }

fun newUserValidation(username: String, email: String, password: String): String? = when {
    username.isBlank() -> "Username is required"
    email.isBlank() -> "Email is required"
    password.length < 8 -> "Password must be at least 8 characters"
    else -> null
}

fun webDavUrl(origin: String?): String = origin?.trimEnd('/')?.plus("/webdav/") ?: "Server URL unavailable"

internal data class PendingAdminAction(
    val title: String,
    val description: String,
    val confirmLabel: String,
    val execute: suspend () -> Unit,
)

internal data class AdminMetric(
    val label: String,
    val value: String,
    val emphasized: Boolean,
)

internal data class AdminPollingState<T>(
    val value: T?,
    val error: String?,
    val refreshing: Boolean,
    val refresh: () -> Unit,
)

@Composable
internal fun <T> rememberAdminPollingState(
    repositoryKey: Any,
    failureMessage: String,
    load: suspend () -> T,
): AdminPollingState<T> {
    var value by remember(repositoryKey) { mutableStateOf<T?>(null) }
    var error by remember(repositoryKey) { mutableStateOf<String?>(null) }
    var refreshing by remember(repositoryKey) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    suspend fun refreshValue() {
        if (refreshing) return
        refreshing = true
        when (val result = runRequest { load() }) {
            is RequestResult.Success -> {
                value = result.response
                error = null
            }
            is RequestResult.Failure -> error = result.error.userMessage(failureMessage)
        }
        refreshing = false
    }

    LaunchedEffect(repositoryKey) {
        while (isActive) {
            refreshValue()
            delay(3_000)
        }
    }

    return AdminPollingState(
        value = value,
        error = error,
        refreshing = refreshing,
        refresh = { scope.launch { refreshValue() } },
    )
}

@Composable
fun AdminScreen(
    repository: AdministrationRepository,
    settingsStore: SettingsStore,
    section: AdminSection,
    currentUserId: Long?,
) {
    val settings by settingsStore.settings.collectAsState(
        initial = Settings(null, false, true, ThemePreference.SYSTEM),
    )

    when (section) {
        AdminSection.IMPORT -> ImportAdministrationScreen(repository, webDavUrl(settings.origin))
        AdminSection.METADATA -> MetadataAdministrationScreen(repository)
        AdminSection.AI -> AiAdministrationScreen(repository)
        AdminSection.USERS -> UserAdministrationScreen(repository, currentUserId)
    }
}

@Composable
internal fun AdminPageScaffold(
    section: AdminSection,
    refreshing: Boolean,
    refresh: () -> Unit,
    content: @Composable () -> Unit,
) {
    MomentoPageScaffold(
        title = section.label,
        subtitle = "Admin",
        backContentDescription = null,
        onBack = null,
        trailingContent = {
            IconButton(onClick = refresh, enabled = !refreshing) {
                if (refreshing) {
                    CircularProgressIndicator(Modifier.padding(12.dp))
                } else {
                    Icon(Icons.Default.Refresh, "Refresh ${section.label}")
                }
            }
        },
        reserveBottomControls = true,
        edgeToEdgeContent = false,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
        Box(Modifier.fillMaxSize().padding(contentPadding)) { content() }
    }
}

@Composable
internal fun ConfirmationDialog(
    action: PendingAdminAction,
    confirm: () -> Unit,
    dismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(action.title) },
        text = { Text(action.description) },
        confirmButton = { TextButton(onClick = confirm) { Text(action.confirmLabel) } },
        dismissButton = { TextButton(onClick = dismiss) { Text("Keep current data") } },
    )
}

@Composable
internal fun AdminPanel(
    title: String,
    description: String,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            Text(description, color = MaterialTheme.colorScheme.onSurfaceVariant)
            content()
        }
    }
}

@Composable
internal fun AdminStatusMetrics(metrics: List<AdminMetric>) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        metrics.chunked(2).forEach { rowMetrics ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                rowMetrics.forEach { metric ->
                    Surface(
                        modifier = Modifier.weight(1f),
                        shape = RoundedCornerShape(12.dp),
                        color = MaterialTheme.colorScheme.surfaceContainer,
                    ) {
                        Column(Modifier.padding(12.dp)) {
                            Text(
                                metric.label.uppercase(),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(
                                metric.value,
                                style = MaterialTheme.typography.titleLarge,
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.Bold,
                                color = if (metric.emphasized) {
                                    MaterialTheme.colorScheme.error
                                } else {
                                    MaterialTheme.colorScheme.onSurface
                                },
                            )
                        }
                    }
                }
                if (rowMetrics.size == 1) Box(Modifier.weight(1f))
            }
        }
    }
}

@Composable
internal fun AdminFailureLog(title: String, entries: List<String>) {
    val logText = if (entries.isEmpty()) "No failures." else entries.joinToString("\n")
    Column(Modifier.fillMaxWidth().padding(top = 16.dp)) {
        Text(
            title.uppercase(),
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Surface(
            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            shape = RoundedCornerShape(12.dp),
            color = MaterialTheme.colorScheme.surfaceContainer,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            SelectionContainer {
                Text(
                    text = logText,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 112.dp, max = 240.dp)
                        .verticalScroll(rememberScrollState())
                        .padding(14.dp),
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
        }
    }
}

@Composable
internal fun AdminError(message: String) {
    Text(
        message,
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodySmall,
        modifier = Modifier.padding(top = 6.dp),
    )
}

@Composable
internal fun LoadingPanel(label: String) {
    Box(Modifier.fillMaxWidth().padding(32.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            CircularProgressIndicator()
            Text(label)
        }
    }
}
