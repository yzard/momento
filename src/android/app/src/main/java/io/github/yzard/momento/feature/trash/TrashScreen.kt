package io.github.yzard.momento.feature.trash

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
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
import io.github.yzard.momento.core.model.TrashMedia
import kotlinx.coroutines.launch

@Composable fun TrashScreen(repository: MomentoRepository) { var items by remember { mutableStateOf<List<TrashMedia>>(emptyList()) }; var selected by remember { mutableStateOf<Set<Long>>(emptySet()) }; var confirm by remember { mutableStateOf<String?>(null) }; val scope = rememberCoroutineScope(); fun refresh() { scope.launch { items = repository.trash(); selected = emptySet() } }; LaunchedEffect(Unit) { refresh() }; Column(Modifier.fillMaxSize()) { TextButton({ confirm = "empty" }) { Text("Empty trash") }; if (selected.isNotEmpty()) { TextButton({ scope.launch { repository.restore(selected.first()); refresh() } }) { Text("Restore") }; TextButton({ confirm = "delete" }) { Text("Delete permanently") } }; LazyColumn { items(items, key = { it.id }) { item -> ListItem({ Text(item.originalFilename) }, supportingContent = { Text("Deleted ${item.deletedAt}") }, modifier = Modifier.clickable { selected = if (item.id in selected) selected - item.id else selected + item.id }) } } }; confirm?.let { action -> AlertDialog(onDismissRequest = { confirm = null }, title = { Text(if (action == "empty") "Empty trash?" else "Delete selected?") }, text = { Text("This cannot be undone.") }, confirmButton = { TextButton({ scope.launch { if (action == "empty") repository.emptyTrash() else selected.forEach { repository.deleteForever(it) }; confirm = null; refresh() } }) { Text("Delete") } }, dismissButton = { TextButton({ confirm = null }) { Text("Cancel") } }) } }
