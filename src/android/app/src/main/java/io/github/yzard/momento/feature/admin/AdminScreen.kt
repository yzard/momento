package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CleaningServices
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.data.RequestResult
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.data.runRequest
import io.github.yzard.momento.core.data.userMessage
import io.github.yzard.momento.core.model.AiStatusResponse
import io.github.yzard.momento.core.model.AiFeatureSchedule
import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiTaskStatus
import io.github.yzard.momento.core.model.ImportStatus
import io.github.yzard.momento.core.model.JobStatus
import io.github.yzard.momento.core.model.User
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

enum class AdminSection(val label: String, val icon: ImageVector) {
    USERS("Users", Icons.Default.People),
    IMPORT("Import", Icons.Default.Folder),
    METADATA("Metadata", Icons.Default.Description),
    AI("AI Processing", Icons.Default.Storage),
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

fun cronFieldsPerRow(widthDp: Int): Int = when {
    widthDp < 420 -> 2
    widthDp < 720 -> 3
    else -> 5
}

fun adminUsesNavigationRail(widthDp: Int): Boolean = widthDp >= 720

fun toggledRole(role: String): String = if (role == "admin") "user" else "admin"

fun statusSummary(status: JobStatus?): String {
    if (status == null) return "Not loaded"
    return "${status.status}: ${status.queuedJobs} queued, ${status.processingJobs} processing, ${status.completedJobs} completed, ${status.failedJobs} failed"
}

fun importStatusSummary(status: ImportStatus?): String {
    if (status == null) return "Not loaded"
    return "${status.status}: ${status.processedFiles}/${status.totalFiles} processed, ${status.successfulImports} imported, ${status.failedImports} failed, ${status.totalMedia} total media"
}

fun aiStatusSummary(status: AiTaskStatus): String =
    "${status.state}: ${status.jobs.queued} queued, ${status.jobs.submitting} submitting, ${status.jobs.submitted} submitted, ${status.jobs.completed} completed, ${status.jobs.failed} failed"

fun isActiveAiState(state: String?): Boolean = state in setOf("queued", "submitting", "submitted", "running", "cancelling")

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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdminScreen(repository: AdministrationRepository, settingsStore: SettingsStore) {
    val settings by settingsStore.settings.collectAsState(
        initial = Settings(null, false, true, ThemePreference.SYSTEM),
    )
    var selectedSection by rememberSaveable { mutableStateOf(AdminSection.USERS) }
    var importStatus by remember { mutableStateOf<ImportStatus?>(null) }
    var metadataStatus by remember { mutableStateOf<JobStatus?>(null) }
    var aiStatus by remember { mutableStateOf<AiStatusResponse?>(null) }
    var importError by remember { mutableStateOf<String?>(null) }
    var metadataError by remember { mutableStateOf<String?>(null) }
    var aiError by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var userRefreshVersion by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()

    suspend fun refreshImport() {
        when (val result = runRequest { repository.importStatus() }) {
            is RequestResult.Success -> {
                importStatus = result.response
                importError = null
            }
            is RequestResult.Failure -> {
                importError = result.error.userMessage("Could not load import status")
            }
        }
    }

    suspend fun refreshMetadata() {
        when (val result = runRequest { repository.metadataStatus() }) {
            is RequestResult.Success -> {
                metadataStatus = result.response
                metadataError = null
            }
            is RequestResult.Failure -> {
                metadataError = result.error.userMessage("Could not load metadata status")
            }
        }
    }

    suspend fun refreshAi() {
        when (val result = runRequest { repository.aiStatus() }) {
            is RequestResult.Success -> {
                aiStatus = result.response
                aiError = null
            }
            is RequestResult.Failure -> {
                aiError = result.error.userMessage("Could not load AI status")
            }
        }
    }

    suspend fun refreshSelected(section: AdminSection) {
        if (refreshing) return
        refreshing = true
        when (section) {
            AdminSection.USERS -> userRefreshVersion += 1
            AdminSection.IMPORT -> refreshImport()
            AdminSection.METADATA -> refreshMetadata()
            AdminSection.AI -> refreshAi()
        }
        refreshing = false
    }

    fun refreshFromUser() {
        scope.launch { refreshSelected(selectedSection) }
    }

    LaunchedEffect(repository, selectedSection) {
        while (isActive) {
            refreshSelected(selectedSection)
            delay(3_000)
        }
    }

    MomentoPageScaffold(
        title = "Admin",
        subtitle = "System access and processing",
        backContentDescription = null,
        onBack = null,
        trailingContent = {
            IconButton(
                onClick = ::refreshFromUser,
                enabled = !refreshing,
            ) {
                if (refreshing) {
                    CircularProgressIndicator(Modifier.padding(12.dp))
                } else {
                    Icon(Icons.Default.Refresh, "Refresh ${selectedSection.label}")
                }
            }
        },
        reserveBottomControls = true,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
        AdminResponsiveLayout(
            selectedSection = selectedSection,
            selectSection = { selectedSection = it },
            modifier = Modifier.fillMaxSize().padding(contentPadding),
        ) {
            AdminSectionContent(
                selectedSection = selectedSection,
                repository = repository,
                webDavUrl = webDavUrl(settings.origin),
                importStatus = importStatus,
                metadataStatus = metadataStatus,
                aiStatus = aiStatus,
                importError = importError,
                metadataError = metadataError,
                aiError = aiError,
                userRefreshVersion = userRefreshVersion,
                refreshImport = { scope.launch { refreshImport() } },
                refreshMetadata = { scope.launch { refreshMetadata() } },
                refreshAi = { scope.launch { refreshAi() } },
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

@Composable
internal fun AdminResponsiveLayout(
    selectedSection: AdminSection,
    selectSection: (AdminSection) -> Unit,
    modifier: Modifier,
    content: @Composable () -> Unit,
) {
    BoxWithConstraints(modifier.fillMaxSize()) {
        AdminSectionLayout(
            selectedSection = selectedSection,
            selectSection = selectSection,
            useNavigationRail = adminUsesNavigationRail(maxWidth.value.toInt()),
            content = content,
        )
    }
}

@Composable
internal fun AdminSectionLayout(
    selectedSection: AdminSection,
    selectSection: (AdminSection) -> Unit,
    useNavigationRail: Boolean,
    content: @Composable () -> Unit,
) {
    if (useNavigationRail) {
        Row(Modifier.fillMaxSize()) {
            AdminSectionRail(selectedSection, selectSection)
            VerticalDivider(Modifier.fillMaxHeight().width(1.dp))
            Box(Modifier.weight(1f).fillMaxHeight()) { content() }
        }
        return
    }

    Column(Modifier.fillMaxSize()) {
        AdminSectionTabs(selectedSection, selectSection)
        Box(Modifier.weight(1f).fillMaxWidth()) { content() }
    }
}

@Composable
private fun AdminSectionRail(
    selectedSection: AdminSection,
    selectSection: (AdminSection) -> Unit,
) {
    NavigationRail(Modifier.fillMaxHeight().width(104.dp)) {
        AdminSection.entries.forEach { section ->
            NavigationRailItem(
                selected = selectedSection == section,
                onClick = { selectSection(section) },
                icon = { Icon(section.icon, null) },
                label = { Text(section.label) },
            )
        }
    }
}

@Composable
internal fun AdminSectionTabs(
    selectedSection: AdminSection,
    selectSection: (AdminSection) -> Unit,
) {
    ScrollableTabRow(selectedTabIndex = selectedSection.ordinal) {
        AdminSection.entries.forEach { section ->
            Tab(
                selected = selectedSection == section,
                onClick = { selectSection(section) },
                icon = { Icon(section.icon, null) },
                text = { Text(section.label) },
            )
        }
    }
}

@Composable
private fun AdminSectionContent(
    selectedSection: AdminSection,
    repository: AdministrationRepository,
    webDavUrl: String,
    importStatus: ImportStatus?,
    metadataStatus: JobStatus?,
    aiStatus: AiStatusResponse?,
    importError: String?,
    metadataError: String?,
    aiError: String?,
    userRefreshVersion: Int,
    refreshImport: () -> Unit,
    refreshMetadata: () -> Unit,
    refreshAi: () -> Unit,
    modifier: Modifier,
) {
    Box(modifier.fillMaxSize()) {
        when (selectedSection) {
            AdminSection.USERS -> UserAdministration(repository, userRefreshVersion)
            AdminSection.IMPORT -> ImportAdministration(repository, webDavUrl, importStatus, importError, refreshImport)
            AdminSection.METADATA -> MetadataAdministration(repository, metadataStatus, metadataError, refreshMetadata)
            AdminSection.AI -> AiAdministration(repository, aiStatus, aiError, refreshAi)
        }
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
