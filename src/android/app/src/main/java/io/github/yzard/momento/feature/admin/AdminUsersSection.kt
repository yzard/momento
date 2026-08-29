package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.model.User
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

@Composable
internal fun UserAdministrationScreen(
    repository: AdministrationRepository,
    currentUserId: Long?,
) {
    var refreshVersion by remember { mutableStateOf(0) }
    AdminPageScaffold(
        section = AdminSection.USERS,
        refreshing = false,
        refresh = { refreshVersion += 1 },
    ) {
        UserAdministration(repository, currentUserId, refreshVersion)
    }
}

@Composable
internal fun UserAdministration(
    repository: AdministrationRepository,
    currentUserId: Long?,
    refreshVersion: Int,
) {
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
        if (active == false && (user.isReserved || user.id == currentUserId)) return
        if (role == "user" && user.id == currentUserId) return
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

    suspend fun deleteUser(user: User) {
        if (user.id in busyUserIds || user.isReserved || user.id == currentUserId) return
        busyUserIds = busyUserIds + user.id
        try {
            repository.deleteUser(user.id)
            loadUsers()
        } catch (_: IOException) {
            error = "Could not delete ${user.username}"
        } catch (_: HttpException) {
            error = "Could not delete ${user.username}"
        } catch (_: SerializationException) {
            error = "Could not delete ${user.username}"
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
            AdminPanel("User Management", "Manage sign-in access and administrator permissions.") {
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
        users?.let { loadedUsers ->
            item {
                UsersTable(
                    users = loadedUsers,
                    currentUserId = currentUserId,
                    busyUserIds = busyUserIds,
                    changeRole = { user, role -> pendingRoleChange = user to role },
                    toggleActive = { user -> scope.launch { updateUser(user, null, !user.isActive) } },
                    delete = { user -> pendingDelete = user },
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
            text = {
                Text(
                    if (role == "admin") {
                        "This user will gain full system access."
                    } else {
                        "This user will lose administrator access."
                    },
                )
            },
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
                    scope.launch { deleteUser(user) }
                }) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { pendingDelete = null }) { Text("Cancel") } },
        )
    }
}

@Composable
internal fun UsersTable(
    users: List<User>,
    currentUserId: Long?,
    busyUserIds: Set<Long>,
    changeRole: (User, String) -> Unit,
    toggleActive: (User) -> Unit,
    delete: (User) -> Unit,
) {
    val columnWidths = listOf(190.dp, 250.dp, 150.dp, 120.dp, 210.dp)
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val tableWidth = maxOf(maxWidth, columnWidths.fold(0.dp) { total, width -> total + width })
        Box(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            Column(
                Modifier
                    .width(tableWidth)
                    .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(12.dp)),
            ) {
                UserTableHeader(columnWidths)
                users.forEach { user ->
                    HorizontalDivider()
                    UserTableRow(
                        user = user,
                        currentUserId = currentUserId,
                        busy = user.id in busyUserIds,
                        columnWidths = columnWidths,
                        changeRole = changeRole,
                        toggleActive = toggleActive,
                        delete = delete,
                    )
                }
            }
        }
    }
}

@Composable
private fun UserTableHeader(columnWidths: List<Dp>) {
    Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surfaceContainer)) {
        listOf("Username", "Email", "Role", "Status", "Actions").forEachIndexed { index, label ->
            Text(
                label,
                modifier = Modifier.width(columnWidths[index]).padding(12.dp),
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun UserTableRow(
    user: User,
    currentUserId: Long?,
    busy: Boolean,
    columnWidths: List<Dp>,
    changeRole: (User, String) -> Unit,
    toggleActive: (User) -> Unit,
    delete: (User) -> Unit,
) {
    val protectedUser = user.isReserved || user.id == currentUserId
    Row(
        modifier = Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surfaceContainerLow),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.width(columnWidths[0]).padding(12.dp)) {
            Text(user.username, fontWeight = FontWeight.SemiBold)
            if (user.isReserved) {
                Text(
                    "RESERVED",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
        Text(user.email, Modifier.width(columnWidths[1]).padding(12.dp))
        Box(Modifier.width(columnWidths[2]).padding(8.dp), contentAlignment = Alignment.CenterStart) {
            OutlinedButton(
                onClick = { changeRole(user, toggledRole(user.role)) },
                enabled = !busy && user.id != currentUserId,
            ) { Text(if (user.role == "admin") "Admin" else "User") }
        }
        Text(
            if (user.isActive) "Active" else "Inactive",
            modifier = Modifier.width(columnWidths[3]).padding(12.dp),
            color = if (user.isActive) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
            fontWeight = FontWeight.SemiBold,
        )
        Row(
            modifier = Modifier.width(columnWidths[4]).padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(
                onClick = { toggleActive(user) },
                enabled = !protectedUser && !busy,
            ) { Text(if (user.isActive) "Deactivate" else "Activate") }
            IconButton(
                onClick = { delete(user) },
                enabled = !protectedUser && !busy,
                modifier = Modifier.semantics { contentDescription = "Delete ${user.username}" },
            ) {
                if (busy) {
                    CircularProgressIndicator(Modifier.width(20.dp))
                } else {
                    Icon(Icons.Default.Delete, null)
                }
            }
        }
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
                OutlinedTextField(
                    username,
                    { username = it },
                    label = { Text("Username") },
                    singleLine = true,
                    enabled = !submitting,
                )
                OutlinedTextField(
                    email,
                    { email = it },
                    label = { Text("Email") },
                    singleLine = true,
                    enabled = !submitting,
                )
                OutlinedTextField(
                    password,
                    { password = it },
                    label = { Text("Temporary password") },
                    supportingText = { Text("At least 8 characters; the user must change it after sign-in") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    enabled = !submitting,
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Administrator")
                    Switch(admin, { admin = it }, enabled = !submitting)
                }
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
                            repository.createUser(
                                username.trim(),
                                email.trim(),
                                password,
                                if (admin) "admin" else "user",
                            )
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
