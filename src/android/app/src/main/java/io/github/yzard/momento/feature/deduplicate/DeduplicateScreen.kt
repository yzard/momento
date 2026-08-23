package io.github.yzard.momento.feature.deduplicate

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ListItem
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
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.DeduplicateGroup
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

fun canManageDeduplication(isAdmin: Boolean): Boolean = isAdmin
@Composable fun DeduplicateScreen(repository: MomentoRepository, isAdmin: Boolean) { var status by remember { mutableStateOf("idle") }; var groups by remember { mutableStateOf<List<DeduplicateGroup>>(emptyList()) }; var selected by remember { mutableStateOf<Set<Long>>(emptySet()) }; var confirm by remember { mutableStateOf(false) }; var error by remember { mutableStateOf<String?>(null) }; val scope = rememberCoroutineScope(); suspend fun load() { runCatching { repository.duplicateGroups() }.onSuccess { groups = it }.onFailure { error = "Could not load duplicate groups" }; if (isAdmin) runCatching { repository.deduplicateStatus().status }.onSuccess { status = it }.onFailure { error = "Could not load status" } }; LaunchedEffect(isAdmin) { load(); while (isAdmin && (status == "running" || status == "queued")) { delay(2000); load() } }; Column { error?.let { Text(it) }; if (canManageDeduplication(isAdmin)) { Text("Status: $status"); Button({ scope.launch { runCatching { repository.startDeduplicate() }.onFailure { error = "Could not start" }; load() } }) { Text("Start") }; Button({ scope.launch { runCatching { repository.cancelDeduplicate() }.onFailure { error = "Could not cancel" }; load() } }) { Text("Cancel") }; Button({ scope.launch { runCatching { repository.cleanDeduplicate() }.onFailure { error = "Could not clean" }; load() } }) { Text("Clean") } }; if (selected.isNotEmpty()) Button({ confirm = true }) { Text("Move selected to trash") }; LazyColumn { items(groups, key = { it.clusterId }) { group -> ListItem({ Text("Group ${group.clusterId}") }, supportingContent = { Text("${group.items.size} candidates") }, modifier = Modifier.clickable { selected = selected + group.items.map { it.id } }) } } }; if (confirm) AlertDialog(onDismissRequest = { confirm = false }, title = { Text("Move to trash?") }, confirmButton = { TextButton({ scope.launch { runCatching { repository.moveToTrash(selected.toList()) }.onSuccess { selected = emptySet(); confirm = false; load() }.onFailure { error = "Could not move media" } } }) { Text("Move") } }, dismissButton = { TextButton({ confirm = false }) { Text("Cancel") } }) }
