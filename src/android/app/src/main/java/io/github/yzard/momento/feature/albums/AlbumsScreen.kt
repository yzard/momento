package io.github.yzard.momento.feature.albums

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Album
import io.github.yzard.momento.core.model.AlbumDetail
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope

@Composable
fun AlbumsScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) {
    var albums by remember { mutableStateOf<List<Album>?>(null) }
    var selected by remember { mutableStateOf<Album?>(null) }
    LaunchedEffect(Unit) { runCatching { repository.albums() }.onSuccess { albums = it } }
    val current = albums
    if (selected != null) {
        AlbumDetailScreen(repository, selected!!.id, { selected = null }, openMedia)
        return
    }
    if (current == null) return Text("Loading albums", Modifier.padding(16.dp))
    if (current.isEmpty()) return Text("No albums yet", Modifier.padding(16.dp))
    LazyColumn(Modifier.fillMaxSize()) { items(current, key = { it.id }) { album ->
        ListItem(
            headlineContent = { Text(album.name) },
            supportingContent = { Text("${album.mediaCount} items") },
            modifier = Modifier.clickable { selected = album },
        )
    } }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AlbumDetailScreen(repository: MomentoRepository, albumId: Long, close: () -> Unit, openMedia: (List<Media>, Int) -> Unit) {
    var detail by remember { mutableStateOf<AlbumDetail?>(null) }
    var selected by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var selectionMode by remember { mutableStateOf(false) }
    var edit by remember { mutableStateOf(false) }
    var add by remember { mutableStateOf(false) }
    var delete by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun reload() { scope.launch { detail = repository.album(albumId); selected = emptySet() } }
    LaunchedEffect(albumId) { reload() }
    val album = detail ?: return Text("Loading album", Modifier.padding(16.dp))
    Column(Modifier.fillMaxSize()) {
        TextButton(close) { Text("Back") }
        Text(album.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(horizontal = 16.dp))
        Text(album.description ?: "", modifier = Modifier.padding(horizontal = 16.dp))
        TextButton({ edit = true }) { Text("Edit") }
        TextButton({ add = true }) { Text("Add media") }
        TextButton({
            selectionMode = !selectionMode
            if (!selectionMode) selected = emptySet()
        }) { Text(if (selectionMode) "Cancel selection" else "Select media") }
        if (selected.isNotEmpty()) {
            TextButton({ scope.launch { repository.removeAlbumMedia(albumId, selected.toList()); reload() } }) { Text("Remove selected") }
            TextButton({ scope.launch { repository.updateAlbum(albumId, null, null, selected.first()); reload() } }) { Text("Use as cover") }
            TextButton({ scope.launch { repository.reorderAlbumMedia(albumId, reorderAlbumIds(album.media.map { it.id }, selected.first(), -1)); reload() } }) { Text("Move earlier") }
            TextButton({ scope.launch { repository.reorderAlbumMedia(albumId, reorderAlbumIds(album.media.map { it.id }, selected.first(), 1)); reload() } }) { Text("Move later") }
        }
        TextButton({ delete = true }) { Text("Delete album") }
        MediaGrid(album.media, repository) { media ->
            if (selectionMode) {
                selected = if (media.id in selected) selected - media.id else selected + media.id
            } else {
                openMedia(album.media, album.media.indexOf(media))
            }
        }
    }
    if (edit) AlbumEditDialog(album, { name, description -> scope.launch { repository.updateAlbum(albumId, name, description, null); edit = false; reload() } }, { edit = false })
    if (add) ModalBottomSheet({ add = false }) { AlbumAddMedia(repository, albumId, { add = false; reload() }) }
    if (delete) AlertDialog(onDismissRequest = { delete = false }, title = { Text("Delete album?") }, text = { Text("Media remains in your library.") }, confirmButton = { TextButton({ scope.launch { repository.deleteAlbum(albumId); close() } }) { Text("Delete") } }, dismissButton = { TextButton({ delete = false }) { Text("Cancel") } })
}

@Composable private fun AlbumAddMedia(repository: MomentoRepository, albumId: Long, complete: () -> Unit) {
    var query by remember { mutableStateOf("") }; var results by remember { mutableStateOf<List<Media>>(emptyList()) }; var picked by remember { mutableStateOf<Set<Long>>(emptySet()) }; val scope = rememberCoroutineScope()
    Column(Modifier.padding(16.dp)) { OutlinedTextField(query, { query = it }, label = { Text("Search library") }); Button({ scope.launch { results = repository.search(query) } }) { Text("Search") }; results.forEach { media -> ListItem({ Text(media.originalFilename) }, modifier = Modifier.clickable { picked = if (media.id in picked) picked - media.id else picked + media.id }) }; Button({ scope.launch { repository.addAlbumMedia(albumId, picked.toList()); complete() } }, enabled = picked.isNotEmpty()) { Text("Add selected") } }
}

@Composable private fun AlbumEditDialog(album: AlbumDetail, save: (String, String?) -> Unit, dismiss: () -> Unit) { var name by remember { mutableStateOf(album.name) }; var description by remember { mutableStateOf(album.description ?: "") }; AlertDialog(onDismissRequest = dismiss, title = { Text("Edit album") }, text = { Column { OutlinedTextField(name, { name = it }, label = { Text("Name") }); OutlinedTextField(description, { description = it }, label = { Text("Description") }) } }, confirmButton = { TextButton({ save(name, description.ifBlank { null }) }) { Text("Save") } }, dismissButton = { TextButton(dismiss) { Text("Cancel") } }) }

fun reorderAlbumIds(ids: List<Long>, selectedId: Long, direction: Int): List<Long> { val index = ids.indexOf(selectedId); val target = (index + direction).coerceIn(0, ids.lastIndex); if (index < 0 || index == target) return ids; return ids.toMutableList().also { val item = it.removeAt(index); it.add(target, item) } }
