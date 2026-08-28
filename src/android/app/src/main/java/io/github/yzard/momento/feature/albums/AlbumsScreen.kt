package io.github.yzard.momento.feature.albums

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.RemoveCircleOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import io.github.yzard.momento.app.designsystem.MomentoActionChip
import io.github.yzard.momento.app.designsystem.MomentoDetailPageHeader
import io.github.yzard.momento.app.designsystem.MomentoFloatingDock
import io.github.yzard.momento.app.designsystem.MomentoPageHeader
import io.github.yzard.momento.app.designsystem.momentoMediaViewerContentPadding
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Album
import io.github.yzard.momento.core.model.AlbumDetail
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.ui.MemoryCardOverlay
import io.github.yzard.momento.feature.media.MediaGrid
import io.github.yzard.momento.feature.media.toggleMediaSelection
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

enum class AlbumCollageLayout { EMPTY, SINGLE, TWO_COLUMNS, LARGE_LEFT, GRID }

fun albumCollageLayout(thumbnailCount: Int): AlbumCollageLayout = when (thumbnailCount) {
    0 -> AlbumCollageLayout.EMPTY
    1 -> AlbumCollageLayout.SINGLE
    2 -> AlbumCollageLayout.TWO_COLUMNS
    3 -> AlbumCollageLayout.LARGE_LEFT
    else -> AlbumCollageLayout.GRID
}

fun albumMemoryCountLabel(mediaCount: Long): String =
    "$mediaCount ${if (mediaCount == 1L) "memory" else "memories"}"

fun removeFromAlbumSelectionLabel(selectedCount: Int): String =
    "Remove from album ($selectedCount)"

fun albumDetailSubtitle(album: AlbumDetail): String =
    listOfNotNull(albumMemoryCountLabel(album.media.size.toLong()), album.description).joinToString(" · ")

@Composable
fun AlbumsScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) {
    var albums by remember { mutableStateOf<List<Album>?>(null) }
    var selectedAlbumId by rememberSaveable { mutableStateOf<Long?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var creating by remember { mutableStateOf(false) }
    var albumPendingDeletion by remember { mutableStateOf<Album?>(null) }
    var working by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    suspend fun loadAlbums() {
        val loaded = executeAlbumOperation { albums = repository.albums() }
        error = if (loaded) null else "Could not load albums"
    }

    LaunchedEffect(Unit) { loadAlbums() }
    BackHandler(enabled = selectedAlbumId != null) { selectedAlbumId = null }

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

    Box(Modifier.fillMaxSize()) {
        when {
            albums == null && error == null -> CircularProgressIndicator(Modifier.align(Alignment.Center))
            error != null -> Column(Modifier.align(Alignment.Center).padding(24.dp)) {
                Text(error!!, color = MaterialTheme.colorScheme.error)
                TextButton({ scope.launch { loadAlbums() } }) { Text("Retry") }
            }
            else -> LazyVerticalGrid(
                columns = GridCells.Adaptive(156.dp),
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 10.dp, top = 88.dp, end = 10.dp, bottom = 88.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (albums.orEmpty().isEmpty()) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Text("No albums yet", Modifier.padding(16.dp))
                    }
                }
                gridItems(albums.orEmpty(), key = { it.id }) { album ->
                    AlbumTile(
                        album = album,
                        repository = repository,
                        enabled = !working,
                        select = { selectedAlbumId = album.id },
                        delete = { albumPendingDeletion = album },
                    )
                }
            }
        }
        MomentoPageHeader(
            title = "Albums",
            subtitle = null,
            modifier = Modifier.align(Alignment.TopStart).windowInsetsPadding(WindowInsets.statusBars),
            leadingContent = null,
            trailingContent = {
                MomentoActionChip(
                    label = "Create album",
                    icon = Icons.Default.Add,
                    enabled = !working,
                    onClick = { creating = true },
                    modifier = Modifier,
                )
            },
        )
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
    albumPendingDeletion?.let { album ->
        AlertDialog(
            onDismissRequest = { if (!working) albumPendingDeletion = null },
            title = { Text("Delete ${album.name}?") },
            text = { Text("Media remains in your library.") },
            confirmButton = {
                TextButton(
                    enabled = !working,
                    onClick = {
                        scope.launch {
                            if (working) return@launch
                            working = true
                            val deleted = executeAlbumOperation { repository.deleteAlbum(album.id) }
                            if (deleted) {
                                albumPendingDeletion = null
                                loadAlbums()
                            } else {
                                error = "Could not delete album"
                            }
                            working = false
                        }
                    },
                ) { Text("Delete") }
            },
            dismissButton = {
                TextButton(
                    enabled = !working,
                    onClick = { albumPendingDeletion = null },
                ) { Text("Cancel") }
            },
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
    var removingSelected by remember(albumId) { mutableStateOf(false) }
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
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            Column(Modifier.align(Alignment.Center).padding(24.dp)) {
                if (error == null) CircularProgressIndicator() else {
                    Text(error!!, color = MaterialTheme.colorScheme.error)
                    TextButton({ scope.launch { loadAlbum() } }) { Text("Retry") }
                }
            }
            MomentoDetailPageHeader(
                title = "Album",
                subtitle = null,
                backContentDescription = "Back to albums",
                enabled = !working,
                onBack = close,
                modifier = Modifier.align(Alignment.TopStart),
            )
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

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        MediaGrid(
            media = album.media,
            repository = repository,
            selectedMediaIds = selectedIds,
            contentPadding = momentoMediaViewerContentPadding,
            headerContent = null,
            footerContent = null,
            modifier = Modifier.fillMaxSize(),
        ) { media ->
            if (selecting) {
                selectedIds = toggleMediaSelection(selectedIds, media.id)
            } else {
                openMedia(album.media, album.media.indexOf(media))
            }
        }
        MomentoDetailPageHeader(
            title = album.name,
            subtitle = albumDetailSubtitle(album),
            backContentDescription = "Back to albums",
            enabled = !working,
            onBack = close,
            modifier = Modifier.align(Alignment.TopStart),
        )
        error?.let { message ->
            Text(
                text = message,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.align(Alignment.TopCenter).padding(top = 92.dp, start = 20.dp, end = 20.dp),
            )
        }
        if (selecting) {
            AlbumSelectionActionDock(
                selectedCount = selectedIds.size,
                working = working,
                remove = { removingSelected = true },
                moveEarlier = {
                    val selectedId = selectedIds.singleOrNull() ?: return@AlbumSelectionActionDock
                    scope.launch {
                        runMutation("Could not reorder media") {
                            repository.reorderAlbumMedia(
                                albumId,
                                reorderAlbumIds(album.media.map { it.id }, selectedId, -1),
                            )
                        }
                    }
                },
                moveLater = {
                    val selectedId = selectedIds.singleOrNull() ?: return@AlbumSelectionActionDock
                    scope.launch {
                        runMutation("Could not reorder media") {
                            repository.reorderAlbumMedia(
                                albumId,
                                reorderAlbumIds(album.media.map { it.id }, selectedId, 1),
                            )
                        }
                    }
                },
                finish = {
                    selecting = false
                    selectedIds = emptySet()
                },
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        } else {
            AlbumPrimaryActionDock(
                enabled = !working,
                select = { selecting = true },
                edit = { editing = true },
                delete = { deleting = true },
                modifier = Modifier.align(Alignment.BottomCenter),
            )
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
    if (removingSelected) {
        AlertDialog(
            onDismissRequest = { if (!working) removingSelected = false },
            title = { Text("Remove ${selectedIds.size} media?") },
            text = { Text("The selected media remains in your library.") },
            confirmButton = {
                TextButton(
                    enabled = !working && selectedIds.isNotEmpty(),
                    onClick = {
                        scope.launch {
                            val removed = runMutation("Could not remove media") {
                                repository.removeAlbumMedia(albumId, selectedIds.toList())
                            }
                            if (removed) removingSelected = false
                        }
                    },
                ) { Text("Remove") }
            },
            dismissButton = {
                TextButton(
                    enabled = !working,
                    onClick = { removingSelected = false },
                ) { Text("Cancel") }
            },
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
                LazyColumn(Modifier.heightIn(max = 420.dp)) {
                    items(albums.orEmpty(), key = { it.id }) { album ->
                        ListItem(
                            headlineContent = { Text(album.name) },
                            supportingContent = { Text("${album.mediaCount} items") },
                            modifier = Modifier.clickable(enabled = !working) { addToAlbum(album.id) },
                        )
                    }
                }
                if (!creating) {
                    MomentoActionChip(
                        label = "Create new album",
                        icon = Icons.Default.Add,
                        enabled = !working,
                        onClick = { creating = true },
                        modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                    )
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
internal fun AlbumPrimaryActionDock(
    enabled: Boolean,
    select: () -> Unit,
    edit: () -> Unit,
    delete: () -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    MomentoFloatingDock(
        modifier = modifier.windowInsetsPadding(WindowInsets.navigationBars).padding(bottom = 12.dp),
    ) {
        TextButton(
            onClick = select,
            enabled = enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
        ) { Text("Select") }
        TextButton(
            onClick = edit,
            enabled = enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
        ) { Text("Edit") }
        TextButton(
            onClick = delete,
            enabled = enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
        ) { Text("Delete") }
    }
}

@Composable
private fun AlbumSelectionActionDock(
    selectedCount: Int,
    working: Boolean,
    remove: () -> Unit,
    moveEarlier: () -> Unit,
    moveLater: () -> Unit,
    finish: () -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    MomentoFloatingDock(
        modifier = modifier.windowInsetsPadding(WindowInsets.navigationBars).padding(bottom = 12.dp),
    ) {
        Text(
            text = "$selectedCount selected",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(start = 10.dp),
        )
        IconButton(onClick = remove, enabled = !working && selectedCount > 0) {
            Icon(Icons.Default.RemoveCircleOutline, "Remove selected media from album")
        }
        IconButton(onClick = moveEarlier, enabled = !working && selectedCount == 1) {
            Icon(Icons.Default.ArrowUpward, "Move selected media earlier")
        }
        IconButton(onClick = moveLater, enabled = !working && selectedCount == 1) {
            Icon(Icons.Default.ArrowDownward, "Move selected media later")
        }
        IconButton(onClick = finish, enabled = !working) {
            Icon(Icons.Default.Done, "Finish selecting media", tint = floatingColors.content)
        }
    }
}

@Composable
private fun AlbumTile(
    album: Album,
    repository: MomentoRepository,
    enabled: Boolean,
    select: () -> Unit,
    delete: () -> Unit,
) {
    val shape = RoundedCornerShape(16.dp)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(1f)
            .clip(shape)
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, shape)
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .semantics {
                contentDescription = "${album.name}, ${albumMemoryCountLabel(album.mediaCount)}"
            }
            .clickable(enabled = enabled, onClick = select),
    ) {
        AlbumThumbnailCollage(album, repository)
        MemoryCardOverlay(
            title = album.name,
            subtitle = null,
            badge = albumMemoryCountLabel(album.mediaCount),
        )
        IconButton(
            onClick = delete,
            enabled = enabled,
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(8.dp)
                .size(40.dp)
                .clip(CircleShape)
                .background(Color.Black.copy(alpha = 0.55f)),
        ) {
            Icon(
                imageVector = Icons.Default.Delete,
                contentDescription = "Delete ${album.name}",
                tint = Color.White,
            )
        }
    }
}

@Composable
private fun AlbumThumbnailCollage(album: Album, repository: MomentoRepository) {
    val context = LocalContext.current
    val thumbnailMediaIds = album.thumbnailMediaIds.take(4)
    val urls by produceState<List<String>>(emptyList(), thumbnailMediaIds) {
        value = thumbnailMediaIds.map { mediaId -> repository.thumbnailUrl(mediaId, true) }
    }

    @Composable
    fun Thumbnail(index: Int, modifier: Modifier) {
        Box(modifier.background(MaterialTheme.colorScheme.surfaceVariant)) {
            urls.getOrNull(index)?.let { thumbnailUrl ->
                AsyncImage(
                    model = ImageRequest.Builder(context).data(thumbnailUrl).build(),
                    imageLoader = repository.authenticatedImageLoader(context),
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }

    when (albumCollageLayout(thumbnailMediaIds.size)) {
        AlbumCollageLayout.EMPTY -> Box(Modifier.fillMaxSize()) {
            Icon(
                imageVector = Icons.Default.Folder,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.45f),
                modifier = Modifier.align(Alignment.Center),
            )
        }
        AlbumCollageLayout.SINGLE -> Thumbnail(0, Modifier.fillMaxSize())
        AlbumCollageLayout.TWO_COLUMNS -> Row(
            Modifier.fillMaxSize().background(MaterialTheme.colorScheme.outlineVariant),
            horizontalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Thumbnail(0, Modifier.weight(1f).fillMaxHeight())
            Thumbnail(1, Modifier.weight(1f).fillMaxHeight())
        }
        AlbumCollageLayout.LARGE_LEFT -> Row(
            Modifier.fillMaxSize().background(MaterialTheme.colorScheme.outlineVariant),
            horizontalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Thumbnail(0, Modifier.weight(1f).fillMaxHeight())
            Column(
                Modifier.weight(1f).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(1.dp),
            ) {
                Thumbnail(1, Modifier.weight(1f).fillMaxWidth())
                Thumbnail(2, Modifier.weight(1f).fillMaxWidth())
            }
        }
        AlbumCollageLayout.GRID -> Column(
            Modifier.fillMaxSize().background(MaterialTheme.colorScheme.outlineVariant),
            verticalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(1.dp)) {
                Thumbnail(0, Modifier.weight(1f).fillMaxHeight())
                Thumbnail(1, Modifier.weight(1f).fillMaxHeight())
            }
            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(1.dp)) {
                Thumbnail(2, Modifier.weight(1f).fillMaxHeight())
                Thumbnail(3, Modifier.weight(1f).fillMaxHeight())
            }
        }
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
