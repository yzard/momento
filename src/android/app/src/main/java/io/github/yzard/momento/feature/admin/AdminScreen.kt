package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.AiStatusResponse
import io.github.yzard.momento.core.model.AiTaskStatus
import io.github.yzard.momento.core.model.ImportStatus
import io.github.yzard.momento.core.model.JobStatus
import io.github.yzard.momento.core.model.User
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException

fun toggledRole(role: String): String = if (role == "admin") "user" else "admin"

fun statusSummary(status: JobStatus?): String {
    if (status == null) return "Not loaded"
    return "${status.status}: ${status.queuedJobs} queued, ${status.processingJobs} processing, ${status.completedJobs} completed, ${status.failedJobs} failed"
}

fun aiStatusSummary(status: AiTaskStatus): String =
    "${status.state}: ${status.jobs.queued} queued, ${status.jobs.submitting} submitting, ${status.jobs.submitted} submitted, ${status.jobs.completed} completed, ${status.jobs.failed} failed"

@Composable
fun AdminScreen(repository: MomentoRepository) {
    var selectedTab by remember { mutableStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        ScrollableTabRow(selectedTabIndex = selectedTab) {
            listOf("Users", "Processing").forEachIndexed { index, label ->
                Tab(selected = selectedTab == index, onClick = { selectedTab = index }, text = { Text(label) })
            }
        }
        if (selectedTab == 0) UserAdministration(repository) else ProcessingAdministration(repository)
    }
}

@Composable
private fun UserAdministration(repository: MomentoRepository) {
    var users by remember { mutableStateOf<List<User>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var createUser by remember { mutableStateOf(false) }
    var deleteUser by remember { mutableStateOf<User?>(null) }
    val scope = rememberCoroutineScope()

    fun loadUsers() {
        scope.launch {
            try {
                users = repository.users()
                error = null
            } catch (_: IOException) {
                error = "Could not load users"
            } catch (_: HttpException) {
                error = "Could not load users"
            }
        }
    }
    LaunchedEffect(Unit) { loadUsers() }
    Column(Modifier.fillMaxSize()) {
        Button({ createUser = true }, Modifier.padding(16.dp)) {
            Icon(Icons.Default.PersonAdd, null)
            Text("Create user")
        }
        error?.let { Text(it, Modifier.padding(horizontal = 16.dp)) }
        LazyColumn {
            items(users.orEmpty(), key = { it.id }) { user ->
                ListItem(
                    headlineContent = { Text(user.username) },
                    supportingContent = { Text("${user.role} · ${user.email}") },
                    modifier = Modifier.clickable {
                        scope.launch {
                            try {
                                repository.updateUser(user.id, toggledRole(user.role), null)
                                loadUsers()
                            } catch (_: IOException) {
                                error = "Could not update ${user.username}"
                            } catch (_: HttpException) {
                                error = "Could not update ${user.username}"
                            }
                        }
                    },
                    trailingContent = {
                        Row {
                            Switch(
                                checked = user.isActive,
                                onCheckedChange = { active ->
                                    scope.launch {
                                        try {
                                            repository.updateUser(user.id, null, active)
                                            loadUsers()
                                        } catch (_: IOException) {
                                            error = "Could not update ${user.username}"
                                        } catch (_: HttpException) {
                                            error = "Could not update ${user.username}"
                                        }
                                    }
                                },
                            )
                            IconButton({ deleteUser = user }) { Icon(Icons.Default.Delete, "Delete ${user.username}") }
                        }
                    },
                )
                HorizontalDivider()
            }
        }
    }
    if (createUser) CreateUserDialog(repository, { createUser = false; loadUsers() }, { error = it })
    deleteUser?.let { user ->
        AlertDialog(
            onDismissRequest = { deleteUser = null },
            title = { Text("Delete ${user.username}?") },
            text = { Text("This account can no longer sign in.") },
            confirmButton = {
                TextButton({
                    scope.launch {
                        try {
                            repository.deleteUser(user.id)
                            deleteUser = null
                            loadUsers()
                        } catch (_: IOException) {
                            error = "Could not delete ${user.username}"
                        } catch (_: HttpException) {
                            error = "Could not delete ${user.username}"
                        }
                    }
                }) { Text("Delete") }
            },
            dismissButton = { TextButton({ deleteUser = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun CreateUserDialog(repository: MomentoRepository, complete: () -> Unit, reportError: (String) -> Unit) {
    var username by remember { mutableStateOf("") }
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var admin by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = complete,
        title = { Text("Create user") },
        text = {
            Column {
                OutlinedTextField(username, { username = it }, label = { Text("Username") })
                OutlinedTextField(email, { email = it }, label = { Text("Email") })
                OutlinedTextField(password, { password = it }, label = { Text("Password") })
                ListItem(headlineContent = { Text("Administrator") }, trailingContent = { Switch(admin, { admin = it }) })
            }
        },
        confirmButton = {
            TextButton({
                if (username.isBlank() || email.isBlank() || password.isBlank()) {
                    reportError("Username, email, and password are required")
                    return@TextButton
                }
                scope.launch {
                    try {
                        repository.createUser(username, email, password, if (admin) "admin" else "user")
                        complete()
                    } catch (_: IOException) {
                        reportError("Could not create user")
                    } catch (_: HttpException) {
                        reportError("Could not create user")
                    }
                }
            }) { Text("Create") }
        },
        dismissButton = { TextButton(complete) { Text("Cancel") } },
    )
}

@Composable
private fun ProcessingAdministration(repository: MomentoRepository) {
    var importStatus by remember { mutableStateOf<ImportStatus?>(null) }
    var metadataStatus by remember { mutableStateOf<JobStatus?>(null) }
    var aiStatus by remember { mutableStateOf<AiStatusResponse?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    fun refresh() {
        scope.launch {
            try {
                importStatus = repository.importStatus()
                metadataStatus = repository.metadataStatus()
                aiStatus = repository.aiStatus()
                error = null
            } catch (_: IOException) {
                error = "Could not load processing status"
            } catch (_: HttpException) {
                error = "Could not load processing status"
            }
        }
    }
    fun runAction(action: suspend () -> Unit) {
        scope.launch {
            try {
                action()
                refresh()
            } catch (_: IOException) {
                error = "Could not update processing"
            } catch (_: HttpException) {
                error = "Could not update processing"
            }
        }
    }
    LaunchedEffect(Unit) { refresh() }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            TextButton({ refresh() }) { Text("Refresh status") }
            error?.let { Text(it, Modifier.padding(horizontal = 16.dp)) }
            Text("Local import", Modifier.padding(16.dp, 12.dp, 16.dp, 0.dp))
            Text(importStatus?.let { "${it.status}: ${it.processedFiles}/${it.totalFiles} processed, ${it.successfulImports} imported, ${it.failedImports} failed" } ?: "Not loaded", Modifier.padding(horizontal = 16.dp))
            TextButton({ runAction { repository.localImport() } }) { Text("Start local import") }
            Text("Metadata", Modifier.padding(16.dp, 12.dp, 16.dp, 0.dp))
            Text(statusSummary(metadataStatus), Modifier.padding(horizontal = 16.dp))
            TextButton({ runAction { repository.generateMetadata() } }) { Text("Generate metadata") }
            TextButton({ runAction { repository.resetMetadata() } }) { Text("Reset metadata") }
            Text("AI", Modifier.padding(16.dp, 12.dp, 16.dp, 0.dp))
            if (aiStatus == null) Text("Not loaded", Modifier.padding(horizontal = 16.dp))
            aiStatus?.tasks?.forEach { status ->
                Text("${status.task}: ${aiStatusSummary(status)}", Modifier.padding(horizontal = 16.dp))
                status.errors.forEach { failure -> Text(failure, Modifier.padding(horizontal = 16.dp)) }
            }
            TextButton({ runAction { repository.startAi() } }) { Text("Start AI") }
            TextButton({ runAction { repository.cancelAi() } }) { Text("Cancel AI") }
            TextButton({ runAction { repository.cleanAi() } }) { Text("Clean AI") }
        }
    }
}
