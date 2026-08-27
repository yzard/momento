package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
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
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.AiStatusResponse
import io.github.yzard.momento.core.model.AiFeatureSchedule
import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiTaskStatus
import io.github.yzard.momento.core.model.ImportStatus
import io.github.yzard.momento.core.model.JobStatus
import io.github.yzard.momento.core.model.User
import kotlinx.coroutines.coroutineScope
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

private data class PendingAdminAction(
    val title: String,
    val description: String,
    val confirmLabel: String,
    val execute: suspend () -> Unit,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdminScreen(repository: MomentoRepository, settingsStore: SettingsStore) {
    val settings by settingsStore.settings.collectAsState(
        initial = Settings(null, false, true, ThemePreference.SYSTEM),
    )
    var selectedSection by remember { mutableStateOf(AdminSection.USERS) }
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
        try {
            importStatus = repository.importStatus()
            importError = null
        } catch (_: IOException) {
            importError = "Could not load import status"
        } catch (_: HttpException) {
            importError = "Could not load import status"
        } catch (_: SerializationException) {
            importError = "Could not load import status"
        }
    }

    suspend fun refreshMetadata() {
        try {
            metadataStatus = repository.metadataStatus()
            metadataError = null
        } catch (_: IOException) {
            metadataError = "Could not load metadata status"
        } catch (_: HttpException) {
            metadataError = "Could not load metadata status"
        } catch (_: SerializationException) {
            metadataError = "Could not load metadata status"
        }
    }

    suspend fun refreshAi() {
        try {
            aiStatus = repository.aiStatus()
            aiError = null
        } catch (_: IOException) {
            aiError = "Could not load AI status"
        } catch (_: HttpException) {
            aiError = "Could not load AI status"
        } catch (_: SerializationException) {
            aiError = "Could not load AI status"
        }
    }

    suspend fun refreshAll() {
        if (refreshing) return
        refreshing = true
        coroutineScope {
            launch { refreshImport() }
            launch { refreshMetadata() }
            launch { refreshAi() }
        }
        refreshing = false
    }

    fun refreshFromUser() {
        userRefreshVersion += 1
        scope.launch { refreshAll() }
    }

    LaunchedEffect(repository) {
        while (isActive) {
            refreshAll()
            delay(3_000)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("Admin")
                        Text(
                            "System access and processing",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                actions = {
                    IconButton(
                        onClick = ::refreshFromUser,
                        enabled = !refreshing,
                    ) {
                        if (refreshing) {
                            CircularProgressIndicator(Modifier.padding(12.dp))
                        } else {
                            Icon(Icons.Default.Refresh, "Refresh all admin status")
                        }
                    }
                },
            )
        },
    ) { scaffoldPadding ->
        BoxWithConstraints(Modifier.fillMaxSize().padding(scaffoldPadding)) {
            val tabletLayout = maxWidth >= 720.dp
            if (tabletLayout) {
                Row(Modifier.fillMaxSize()) {
                    NavigationRail(Modifier.width(104.dp)) {
                        AdminSection.entries.forEach { section ->
                            NavigationRailItem(
                                selected = selectedSection == section,
                                onClick = { selectedSection = section },
                                icon = { Icon(section.icon, null) },
                                label = { Text(section.label) },
                            )
                        }
                    }
                    VerticalDivider(Modifier.fillMaxSize().width(1.dp))
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
                        refreshing = refreshing,
                        userRefreshVersion = userRefreshVersion,
                        refresh = ::refreshFromUser,
                        refreshImport = { scope.launch { refreshImport() } },
                        refreshMetadata = { scope.launch { refreshMetadata() } },
                        refreshAi = { scope.launch { refreshAi() } },
                        modifier = Modifier.weight(1f),
                    )
                }
            } else {
                Column(Modifier.fillMaxSize()) {
                    AdminSectionTabs(selectedSection) { selectedSection = it }
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
                        refreshing = refreshing,
                        userRefreshVersion = userRefreshVersion,
                        refresh = ::refreshFromUser,
                        refreshImport = { scope.launch { refreshImport() } },
                        refreshMetadata = { scope.launch { refreshMetadata() } },
                        refreshAi = { scope.launch { refreshAi() } },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AdminSectionContent(
    selectedSection: AdminSection,
    repository: MomentoRepository,
    webDavUrl: String,
    importStatus: ImportStatus?,
    metadataStatus: JobStatus?,
    aiStatus: AiStatusResponse?,
    importError: String?,
    metadataError: String?,
    aiError: String?,
    refreshing: Boolean,
    userRefreshVersion: Int,
    refresh: () -> Unit,
    refreshImport: () -> Unit,
    refreshMetadata: () -> Unit,
    refreshAi: () -> Unit,
    modifier: Modifier,
) {
    PullToRefreshBox(
        isRefreshing = refreshing,
        onRefresh = refresh,
        modifier = modifier.fillMaxSize(),
    ) {
        when (selectedSection) {
            AdminSection.USERS -> UserAdministration(repository, userRefreshVersion)
            AdminSection.IMPORT -> ImportAdministration(repository, webDavUrl, importStatus, importError, refreshImport)
            AdminSection.METADATA -> MetadataAdministration(repository, metadataStatus, metadataError, refreshMetadata)
            AdminSection.AI -> AiAdministration(repository, aiStatus, aiError, refreshAi)
        }
    }
}

@Composable
private fun UserAdministration(repository: MomentoRepository, refreshVersion: Int) {
    var users by remember(repository) { mutableStateOf<List<User>?>(null) }
    var error by remember(repository) { mutableStateOf<String?>(null) }
    var createUser by remember { mutableStateOf(false) }
    var pendingRoleChange by remember { mutableStateOf<Pair<User, String>?>(null) }
    var pendingDelete by remember { mutableStateOf<User?>(null) }
    var busyUserIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    val scope = rememberCoroutineScope()

    suspend fun loadUsers() {
        try {
            users = repository.users()
            error = null
        } catch (_: IOException) {
            error = "Could not load users"
        } catch (_: HttpException) {
            error = "Could not load users"
        } catch (_: SerializationException) {
            error = "Could not load users"
        }
    }

    suspend fun updateUser(user: User, role: String?, active: Boolean?) {
        if (user.id in busyUserIds) return
        busyUserIds = busyUserIds + user.id
        try {
            repository.updateUser(user.id, role, active)
            loadUsers()
        } catch (_: IOException) {
            error = "Could not update ${user.username}"
        } catch (_: HttpException) {
            error = "Could not update ${user.username}"
        } catch (_: SerializationException) {
            error = "Could not update ${user.username}"
        } finally {
            busyUserIds = busyUserIds - user.id
        }
    }

    LaunchedEffect(repository, refreshVersion) { loadUsers() }
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            AdminPanel("Users", "Manage sign-in access and administrator permissions.") {
                Button(onClick = { createUser = true }) {
                    Icon(Icons.Default.PersonAdd, null)
                    Text("Create user", Modifier.padding(start = 8.dp))
                }
                error?.let { AdminError(it) }
            }
        }
        if (users == null && error == null) {
            item { LoadingPanel("Loading users") }
        }
        items(users.orEmpty(), key = { it.id }) { user ->
            Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)) {
                ListItem(
                    headlineContent = { Text(user.username, fontWeight = FontWeight.SemiBold) },
                    supportingContent = { Text(user.email) },
                    trailingContent = {
                        IconButton(
                            onClick = { pendingDelete = user },
                            enabled = user.id !in busyUserIds,
                        ) { Icon(Icons.Default.Delete, "Delete ${user.username}") }
                    },
                )
                HorizontalDivider()
                ListItem(
                    headlineContent = { Text("Administrator") },
                    supportingContent = { Text("Can manage users, imports, metadata, and AI jobs") },
                    trailingContent = {
                        Switch(
                            checked = user.role == "admin",
                            onCheckedChange = { checked ->
                                pendingRoleChange = user to if (checked) "admin" else "user"
                            },
                            enabled = user.id !in busyUserIds,
                        )
                    },
                )
                ListItem(
                    headlineContent = { Text("Account active") },
                    supportingContent = { Text(if (user.isActive) "Sign-in is allowed" else "Sign-in is blocked") },
                    trailingContent = {
                        Switch(
                            checked = user.isActive,
                            onCheckedChange = { active -> scope.launch { updateUser(user, null, active) } },
                            enabled = user.id !in busyUserIds,
                        )
                    },
                )
            }
        }
    }

    if (createUser) {
        CreateUserDialog(
            repository = repository,
            dismiss = { createUser = false },
            complete = {
                createUser = false
                scope.launch { loadUsers() }
            },
        )
    }
    pendingRoleChange?.let { (user, role) ->
        AlertDialog(
            onDismissRequest = { pendingRoleChange = null },
            title = { Text("Change ${user.username}'s permission?") },
            text = { Text(if (role == "admin") "This user will gain full system access." else "This user will lose administrator access.") },
            confirmButton = {
                TextButton(onClick = {
                    pendingRoleChange = null
                    scope.launch { updateUser(user, role, null) }
                }) { Text("Change permission") }
            },
            dismissButton = { TextButton(onClick = { pendingRoleChange = null }) { Text("Cancel") } },
        )
    }
    pendingDelete?.let { user ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            title = { Text("Delete ${user.username}?") },
            text = { Text("This account can no longer sign in. Existing media is not deleted.") },
            confirmButton = {
                TextButton(onClick = {
                    pendingDelete = null
                    scope.launch {
                        busyUserIds = busyUserIds + user.id
                        try {
                            repository.deleteUser(user.id)
                            loadUsers()
                        } catch (_: IOException) {
                            error = "Could not delete ${user.username}"
                        } catch (_: HttpException) {
                            error = "Could not delete ${user.username}"
                        } finally {
                            busyUserIds = busyUserIds - user.id
                        }
                    }
                }) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { pendingDelete = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun CreateUserDialog(
    repository: MomentoRepository,
    dismiss: () -> Unit,
    complete: () -> Unit,
) {
    var username by remember { mutableStateOf("") }
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var admin by remember { mutableStateOf(false) }
    var submitting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = { if (!submitting) dismiss() },
        title = { Text("Create user") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                OutlinedTextField(username, { username = it }, label = { Text("Username") }, singleLine = true, enabled = !submitting)
                OutlinedTextField(email, { email = it }, label = { Text("Email") }, singleLine = true, enabled = !submitting)
                OutlinedTextField(
                    password,
                    { password = it },
                    label = { Text("Temporary password") },
                    supportingText = { Text("At least 8 characters; the user must change it after sign-in") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    enabled = !submitting,
                )
                ListItem(
                    headlineContent = { Text("Administrator") },
                    trailingContent = { Switch(admin, { admin = it }, enabled = !submitting) },
                )
                error?.let { AdminError(it) }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    val validation = newUserValidation(username, email, password)
                    if (validation != null) {
                        error = validation
                        return@TextButton
                    }
                    scope.launch {
                        submitting = true
                        try {
                            repository.createUser(username.trim(), email.trim(), password, if (admin) "admin" else "user")
                            complete()
                        } catch (_: IOException) {
                            error = "Could not create user"
                        } catch (_: HttpException) {
                            error = "Could not create user"
                        } catch (_: SerializationException) {
                            error = "Could not create user"
                        } finally {
                            submitting = false
                        }
                    }
                },
                enabled = !submitting,
            ) { Text(if (submitting) "Creating" else "Create") }
        },
        dismissButton = { TextButton(onClick = dismiss, enabled = !submitting) { Text("Cancel") } },
    )
}

@Composable
private fun ImportAdministration(
    repository: MomentoRepository,
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
private fun MetadataAdministration(
    repository: MomentoRepository,
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

@Composable
private fun AiAdministration(
    repository: MomentoRepository,
    status: AiStatusResponse?,
    error: String?,
    refresh: () -> Unit,
) {
    val controls = listOf<Pair<AdminAiFeature?, String>>(null to "All AI jobs") +
        AdminAiFeature.entries.map { it to it.label }
    var busyControl by remember { mutableStateOf<String?>(null) }
    var actionError by remember { mutableStateOf<String?>(null) }
    var pendingAction by remember { mutableStateOf<PendingAdminAction?>(null) }
    val scope = rememberCoroutineScope()

    fun taskState(feature: AdminAiFeature?): String? {
        if (feature == null) {
            return if (
                AdminAiFeature.entries.any { currentFeature ->
                    isActiveAiState(taskState(currentFeature))
                }
            ) "running" else "idle"
        }
        if (feature == AdminAiFeature.DEDUPLICATE) return status?.deduplicate?.status
        return status?.tasks?.firstOrNull { it.task == feature.identifier }?.state
    }

    fun runAction(
        controlKey: String,
        actionLabel: String,
        action: suspend () -> Unit,
    ) {
        if (busyControl != null) return
        scope.launch {
            busyControl = controlKey
            try {
                action()
                actionError = null
                refresh()
            } catch (_: IOException) {
                actionError = "$actionLabel failed"
            } catch (_: HttpException) {
                actionError = "$actionLabel failed"
            } catch (_: SerializationException) {
                actionError = "$actionLabel failed"
            } finally {
                busyControl = null
            }
        }
    }

    fun actionFor(feature: AdminAiFeature?, actionName: String): suspend () -> Unit = {
        when (actionName) {
            "start" -> if (feature == null) repository.startAi() else repository.startAiFeature(feature.identifier)
            "cancel" -> if (feature == null) repository.cancelAi() else repository.cancelAiFeature(feature.identifier)
            "clean" -> if (feature == null) repository.cleanAi() else repository.cleanAiFeature(feature.identifier)
            else -> error("Unknown AI action $actionName")
        }
        Unit
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            AdminPanel("AI processing", "Each task runs independently through the durable server queue.") {
                status?.let { currentStatus ->
                    Text("${currentStatus.faceGroups} face groups · ${currentStatus.deduplicate.clustersCreated} duplicate groups")
                }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
            }
        }
        item {
            AdminPanel("AI work status", "Queued, submitting, submitted, failed, and completed jobs by feature.") {
                AiStatusTable(status)
            }
        }
        items(controls, key = { it.second }) { (feature, label) ->
            val state = taskState(feature)
            val running = isActiveAiState(state)
            val controlKey = feature?.identifier ?: "all"
            Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)) {
                Column(Modifier.fillMaxWidth().padding(16.dp)) {
                    Text(label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Text(state ?: "Not loaded", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (feature != null && feature != AdminAiFeature.DEDUPLICATE) {
                        status?.tasks?.firstOrNull { it.task == feature.identifier }?.let { task ->
                            Text(aiStatusSummary(task), style = MaterialTheme.typography.bodySmall)
                            task.errors.forEach { AdminError(it) }
                        }
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Button(
                            onClick = {
                                if (running) {
                                    pendingAction = PendingAdminAction(
                                        title = "Cancel $label?",
                                        description = "Queued and active work for this control will be cancelled.",
                                        confirmLabel = "Cancel jobs",
                                        execute = {
                                            runAction(controlKey, "Cancel $label", actionFor(feature, "cancel"))
                                        },
                                    )
                                } else {
                                    runAction(controlKey, "Start $label", actionFor(feature, "start"))
                                }
                            },
                            enabled = busyControl == null,
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(if (running) Icons.Default.Stop else Icons.Default.PlayArrow, null)
                            Text(if (busyControl == controlKey) "Working" else if (running) "Cancel" else "Start")
                        }
                        OutlinedButton(
                            onClick = {
                                pendingAction = PendingAdminAction(
                                    title = "Clean $label data?",
                                    description = "Stored results and eligible job state for this control will be removed.",
                                    confirmLabel = "Clean data",
                                    execute = {
                                        runAction(controlKey, "Clean $label", actionFor(feature, "clean"))
                                    },
                                )
                            },
                            enabled = !running && busyControl == null,
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Default.CleaningServices, null)
                            Text("Clean")
                        }
                    }
                    if (feature != null) {
                        status?.schedules?.firstOrNull { it.feature == feature.identifier }?.let { schedule ->
                            AiScheduleEditor(
                                label = label,
                                schedule = schedule,
                                busy = busyControl != null,
                                save = { cronExpression ->
                                    runAction("${controlKey}-schedule", "Save $label schedule") {
                                        repository.updateAiSchedule(feature.identifier, cronExpression)
                                        Unit
                                    }
                                },
                            )
                        }
                    }
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

@Composable
private fun AiStatusTable(status: AiStatusResponse?) {
    Column(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .width(720.dp),
    ) {
        AiStatusRow("Feature", listOf("Queued", "Submitting", "Submitted", "Failed", "Completed"), true)
        HorizontalDivider(Modifier.padding(vertical = 6.dp))
        AdminAiFeature.entries.forEach { feature ->
            val jobs = aiJobCounts(status, feature)
            AiStatusRow(
                feature.label,
                listOf(
                    jobs?.queued?.toString() ?: "0",
                    jobs?.submitting?.toString() ?: "0",
                    jobs?.submitted?.toString() ?: "0",
                    jobs?.failed?.toString() ?: "0",
                    jobs?.completed?.toString() ?: "0",
                ),
                false,
            )
        }
    }
}

@Composable
private fun AiStatusRow(label: String, values: List<String>, header: Boolean) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(
            label,
            modifier = Modifier.width(180.dp),
            fontWeight = if (header) FontWeight.Bold else FontWeight.SemiBold,
            style = MaterialTheme.typography.bodySmall,
        )
        values.forEach { value ->
            Text(
                value,
                modifier = Modifier.width(108.dp),
                textAlign = TextAlign.End,
                fontWeight = if (header) FontWeight.Bold else FontWeight.Normal,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun AiScheduleEditor(
    label: String,
    schedule: AiFeatureSchedule,
    busy: Boolean,
    save: (String) -> Unit,
) {
    var cronExpression by remember(schedule.cronExpression) { mutableStateOf(schedule.cronExpression) }
    val normalizedExpression = cronExpression.trim().replace(Regex("\\s+"), " ")
    Column(Modifier.fillMaxWidth().padding(top = 12.dp)) {
        Text("Schedule · five-field cron · system timezone", style = MaterialTheme.typography.labelSmall)
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                value = cronExpression,
                onValueChange = { cronExpression = it },
                label = { Text("$label cron schedule") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            OutlinedButton(
                onClick = { save(cronExpression) },
                enabled = !busy && normalizedExpression.isNotEmpty() && normalizedExpression != schedule.cronExpression,
            ) {
                Text("Save")
            }
        }
    }
}

@Composable
private fun ConfirmationDialog(
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
private fun AdminPanel(
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
private fun AdminError(message: String) {
    Text(
        message,
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodySmall,
        modifier = Modifier.padding(top = 6.dp),
    )
}

@Composable
private fun LoadingPanel(label: String) {
    Box(Modifier.fillMaxWidth().padding(32.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            CircularProgressIndicator()
            Text(label)
        }
    }
}
