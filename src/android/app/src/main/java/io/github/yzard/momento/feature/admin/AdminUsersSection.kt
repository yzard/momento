package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.model.User
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

@Composable
internal fun UserAdministration(repository: AdministrationRepository, refreshVersion: Int) {
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
    repository: AdministrationRepository,
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
