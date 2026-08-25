package io.github.yzard.momento.feature.albums

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Album
import io.github.yzard.momento.core.model.AlbumDetail
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

@Composable
fun AlbumsScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) {
    var albums by remember { mutableStateOf<List<Album>?>(null) }
    var selectedAlbumId by remember { mutableStateOf<Long?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var creating by remember { mutableStateOf(false) }
    var working by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    suspend fun loadAlbums() {
        val loaded = executeAlbumOperation { albums = repository.albums() }
        error = if (loaded) null else "Could not load albums"
    }

    LaunchedEffect(Unit) { loadAlbums() }

    val albumId = selectedAlbumId
    if (albumId != null) {
        AlbumDetailScreen(
            repository,
            albumId,
            {
                selectedAlbumId = null
                scope.launch { loadAlbums() }
            },
            openMedia,
        )
        return
    }

    when {
        albums == null && error == null -> CircularProgressIndicator(Modifier.padding(24.dp))
        error != null -> Column(Modifier.padding(24.dp)) {
            Text(error!!, color = MaterialTheme.colorScheme.error)
            TextButton({ scope.launch { loadAlbums() } }) { Text("Retry") }
        }
        else -> LazyColumn(Modifier.fillMaxSize()) {
            item {
                Button(
                    onClick = { creating = true },
                    enabled = !working,
                    modifier = Modifier.padding(16.dp),
                ) { Text("Create album") }
            }
            if (albums.orEmpty().isEmpty()) {
                item { Text("No albums yet", Modifier.padding(16.dp)) }
            }
            items(albums.orEmpty(), key = { it.id }) { album ->
                ListItem(
                    headlineContent = { Text(album.name) },
                    supportingContent = { Text("${album.mediaCount} items") },
                    trailingContent = {
                        TextButton(
                            enabled = !working,
                            onClick = {
                                scope.launch {
                                    if (working) return@launch
                                    working = true
                                    val deleted = executeAlbumOperation { repository.deleteAlbum(album.id) }
                                    if (deleted) loadAlbums() else error = "Could not delete album"
                                    working = false
                                }
                            },
                        ) { Text("Delete") }
                    },
                    modifier = Modifier.clickable(enabled = !working) { selectedAlbumId = album.id },
                )
                HorizontalDivider()
            }
        }
    }

    if (creating) {
        AlbumEditorDialog(
            initialName = "",
            initialDescription = "",
            confirmLabel = "Create",
            working = working,
            save = { name, description ->
                scope.launch {
                    if (working) return@launch
                    working = true
                    val created = executeAlbumOperation { repository.createAlbum(name, description) }
                    if (created) {
                        creating = false
                        loadAlbums()
                    } else {
                        error = "Could not create album"
                    }
                    working = false
                }
            },
            dismiss = { creating = false },
        )
    }
}

@Composable
private fun AlbumDetailScreen(
    repository: MomentoRepository,
    albumId: Long,
    close: () -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var detail by remember(albumId) { mutableStateOf<AlbumDetail?>(null) }
    var selectedIds by remember(albumId) { mutableStateOf<Set<Long>>(emptySet()) }
    var selecting by remember(albumId) { mutableStateOf(false) }
    var editing by remember(albumId) { mutableStateOf(false) }
    var deleting by remember(albumId) { mutableStateOf(false) }
    var error by remember(albumId) { mutableStateOf<String?>(null) }
    var working by remember(albumId) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    suspend fun loadAlbum() {
        val loaded = executeAlbumOperation { detail = repository.album(albumId) }
        if (loaded) {
            selectedIds = emptySet()
            error = null
        } else {
            error = "Could not load album"
        }
    }

    LaunchedEffect(albumId) { loadAlbum() }
    val album = detail
    if (album == null) {
        Column(Modifier.padding(24.dp)) {
            if (error == null) CircularProgressIndicator() else {
                Text(error!!, color = MaterialTheme.colorScheme.error)
                TextButton({ scope.launch { loadAlbum() } }) { Text("Retry") }
            }
        }
        return
    }

    suspend fun runMutation(failure: String, action: suspend () -> Unit): Boolean {
        if (working) return false
        working = true
        val succeeded = executeAlbumOperation(action)
        if (succeeded) loadAlbum() else error = failure
        working = false
        return succeeded
    }

    Column(Modifier.fillMaxSize()) {
        TextButton(close, enabled = !working) { Text("Back") }
        Text(album.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(horizontal = 16.dp))
        album.description?.let { Text(it, modifier = Modifier.padding(horizontal = 16.dp)) }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(16.dp)) }
        TextButton({ editing = true }, enabled = !working) { Text("Edit") }
        TextButton({ selecting = !selecting; if (!selecting) selectedIds = emptySet() }, enabled = !working) {
            Text(if (selecting) "Cancel selection" else "Select media")
        }
        if (selectedIds.isNotEmpty()) {
            TextButton({ scope.launch { runMutation("Could not remove media") { repository.removeAlbumMedia(albumId, selectedIds.toList()) } } }, enabled = !working) { Text("Remove selected") }
            selectedIds.singleOrNull()?.let { selectedId ->
                TextButton({ scope.launch { runMutation("Could not update album cover") { repository.updateAlbum(albumId, null, null, selectedId) } } }, enabled = !working) { Text("Use as cover") }
                TextButton({ scope.launch { runMutation("Could not reorder media") { repository.reorderAlbumMedia(albumId, reorderAlbumIds(album.media.map { it.id }, selectedId, -1)) } } }, enabled = !working) { Text("Move earlier") }
                TextButton({ scope.launch { runMutation("Could not reorder media") { repository.reorderAlbumMedia(albumId, reorderAlbumIds(album.media.map { it.id }, selectedId, 1)) } } }, enabled = !working) { Text("Move later") }
            }
        }
        TextButton({ deleting = true }, enabled = !working) { Text("Delete album") }
        MediaGrid(album.media, repository) { media ->
            if (selecting) {
                selectedIds = if (media.id in selectedIds) selectedIds - media.id else selectedIds + media.id
            } else {
                openMedia(album.media, album.media.indexOf(media))
            }
        }
    }

    if (editing) {
        AlbumEditorDialog(
            initialName = album.name,
            initialDescription = album.description.orEmpty(),
            confirmLabel = "Save",
            working = working,
            save = { name, description ->
                scope.launch {
                    val updated = runMutation("Could not update album") {
                        repository.updateAlbum(albumId, name, description, null)
                    }
                    if (updated) editing = false
                }
            },
            dismiss = { editing = false },
        )
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text("Delete album?") },
            text = { Text("Media remains in your library.") },
            confirmButton = {
                TextButton(
                    enabled = !working,
                    onClick = {
                        scope.launch {
                            if (working) return@launch
                            working = true
                            val deleted = executeAlbumOperation { repository.deleteAlbum(albumId) }
                            if (deleted) {
                                deleting = false
                                close()
                            } else {
                                error = "Could not delete album"
                            }
                            working = false
                        }
                    },
                ) { Text("Delete") }
            },
            dismissButton = { TextButton({ deleting = false }, enabled = !working) { Text("Cancel") } },
        )
    }
}

@Composable
fun AlbumAddMediaSheet(
    repository: MomentoRepository,
    mediaIds: List<Long>,
    close: () -> Unit,
) {
    var albums by remember(mediaIds) { mutableStateOf<List<Album>?>(null) }
    var error by remember(mediaIds) { mutableStateOf<String?>(null) }
    var creating by remember(mediaIds) { mutableStateOf(false) }
    var working by remember(mediaIds) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun fail(message: String) { error = message; working = false }
    fun addToAlbum(albumId: Long) {
        scope.launch {
            working = true
            try {
                repository.addAlbumMedia(albumId, mediaIds)
                close()
            } catch (_: IOException) {
                fail("Could not add media to album")
            } catch (_: HttpException) {
                fail("Could not add media to album")
            } catch (_: SerializationException) {
                fail("Could not add media to album")
            }
        }
    }

    LaunchedEffect(mediaIds) {
        try {
            albums = repository.albums()
        } catch (_: IOException) {
            error = "Could not load albums"
        } catch (_: HttpException) {
            error = "Could not load albums"
        } catch (_: SerializationException) {
            error = "Could not load albums"
        }
    }

    Column(Modifier.fillMaxWidth().padding(bottom = 20.dp)) {
        Text("Add ${mediaIds.size} item${if (mediaIds.size == 1) "" else "s"} to album", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(20.dp))
        error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 20.dp)) }
        when {
            albums == null -> CircularProgressIndicator(Modifier.padding(20.dp))
            else -> {
                LazyColumn {
                    items(albums.orEmpty(), key = { it.id }) { album ->
                        ListItem(
                            headlineContent = { Text(album.name) },
                            supportingContent = { Text("${album.mediaCount} items") },
                            modifier = Modifier.clickable(enabled = !working) { addToAlbum(album.id) },
                        )
                    }
                }
                if (!creating) {
                    TextButton(onClick = { creating = true }, modifier = Modifier.padding(horizontal = 12.dp)) { Text("Create new album") }
                }
            }
        }
    }
    if (creating) {
        AlbumEditorDialog(
            initialName = "",
            initialDescription = "",
            confirmLabel = "Create and add",
            working = working,
            save = { name, description ->
                scope.launch {
                    working = true
                    try {
                        val album = repository.createAlbum(name, description)
                        repository.addAlbumMedia(album.id, mediaIds)
                        close()
                    } catch (_: IOException) {
                        fail("Could not create album")
                    } catch (_: HttpException) {
                        fail("Could not create album")
                    } catch (_: SerializationException) {
                        fail("Could not create album")
                    }
                }
            },
            dismiss = { creating = false },
        )
    }
}

@Composable
private fun AlbumEditorDialog(
    initialName: String,
    initialDescription: String,
    confirmLabel: String,
    working: Boolean,
    save: (String, String?) -> Unit,
    dismiss: () -> Unit,
) {
    var name by remember { mutableStateOf(initialName) }
    var description by remember { mutableStateOf(initialDescription) }
    AlertDialog(
        onDismissRequest = { if (!working) dismiss() },
        title = { Text(if (initialName.isEmpty()) "New album" else "Edit album") },
        text = {
            Column {
                OutlinedTextField(name, { name = it }, label = { Text("Name") })
                OutlinedTextField(description, { description = it }, label = { Text("Description") })
            }
        },
        confirmButton = { TextButton({ if (name.isNotBlank()) save(name.trim(), description.ifBlank { null }) }, enabled = name.isNotBlank() && !working) { Text(confirmLabel) } },
        dismissButton = { TextButton(dismiss, enabled = !working) { Text("Cancel") } },
    )
}

fun reorderAlbumIds(ids: List<Long>, selectedId: Long, direction: Int): List<Long> {
    val index = ids.indexOf(selectedId)
    if (index < 0 || ids.size < 2) return ids
    val target = (index + direction).coerceIn(0, ids.lastIndex)
    if (index == target) return ids
    return ids.toMutableList().also { items ->
        val selected = items.removeAt(index)
        items.add(target, selected)
    }
}

suspend fun executeAlbumOperation(action: suspend () -> Unit): Boolean = try {
    action()
    true
} catch (_: IOException) {
    false
} catch (_: HttpException) {
    false
} catch (_: SerializationException) {
    false
}
