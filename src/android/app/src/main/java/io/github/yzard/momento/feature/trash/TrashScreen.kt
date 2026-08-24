package io.github.yzard.momento.feature.trash

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DeleteForever
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
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
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TrashMedia
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.SelectableMediaThumbnail
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.toggleMediaSelection
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

private enum class TrashAction { DELETE_SELECTED, EMPTY }

@Composable
fun TrashScreen(repository: MomentoRepository) {
    var trashItems by remember { mutableStateOf<List<TrashMedia>?>(null) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var selecting by remember { mutableStateOf(false) }
    var confirmAction by remember { mutableStateOf<TrashAction?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var working by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val gridState = rememberLazyGridState()

    suspend fun refresh() {
        try {
            trashItems = repository.trash()
            selectedIds = emptySet()
            selecting = false
            error = null
        } catch (_: IOException) {
            error = "Could not load Trash"
        } catch (_: HttpException) {
            error = "Could not load Trash"
        } catch (_: SerializationException) {
            error = "Could not load Trash"
        }
    }

    suspend fun perform(action: TrashAction) {
        if (action == TrashAction.DELETE_SELECTED && selectedIds.isEmpty()) return
        confirmAction = null
        working = true
        try {
            when (action) {
                TrashAction.DELETE_SELECTED -> repository.deleteForever(selectedIds.toList())
                TrashAction.EMPTY -> repository.emptyTrash()
            }
            refresh()
        } catch (_: IOException) {
            error = "Could not update Trash"
        } catch (_: HttpException) {
            error = "Could not update Trash"
        } catch (_: SerializationException) {
            error = "Could not update Trash"
        } finally {
            working = false
        }
    }

    suspend fun restoreSelected() {
        if (selectedIds.isEmpty()) return
        working = true
        try {
            repository.restore(selectedIds.toList())
            refresh()
        } catch (_: IOException) {
            error = "Could not restore the selected media"
        } catch (_: HttpException) {
            error = "Could not restore the selected media"
        } catch (_: SerializationException) {
            error = "Could not restore the selected media"
        } finally {
            working = false
        }
    }

    LaunchedEffect(repository) { refresh() }

    val current = trashItems
    when {
        current == null && error != null -> ErrorState(error!!) { scope.launch { refresh() } }
        current == null -> LoadingState()
        current.isEmpty() -> EmptyState("Trash is empty")
        else -> {
            val media = remember(current) { current.map(TrashMedia::asMedia) }
            BoxWithConstraints(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
                val columns = adaptiveGridColumns(maxWidth.value.toInt())
                LazyVerticalGrid(
                    columns = GridCells.Fixed(columns),
                    state = gridState,
                    contentPadding = PaddingValues(bottom = 104.dp),
                ) {
                    items(media, key = { it.id }) { item ->
                        SelectableMediaThumbnail(
                            media = item,
                            repository = repository,
                            trashed = true,
                            selected = item.id in selectedIds,
                            modifier = Modifier
                                .fillMaxWidth()
                                .aspectRatio(1f)
                                .padding(0.5.dp)
                                .background(MaterialTheme.colorScheme.surfaceVariant)
                                .clickable {
                                    selecting = true
                                    selectedIds = toggleMediaSelection(selectedIds, item.id)
                                },
                        )
                    }
                }
                TrashSelectionControl(
                    selecting = selecting,
                    selectedCount = selectedIds.size,
                    cancel = {
                        selecting = false
                        selectedIds = emptySet()
                    },
                    start = { selecting = true },
                    modifier = Modifier.align(Alignment.TopEnd).padding(end = 12.dp),
                )
                if (selecting && selectedIds.isNotEmpty()) {
                    TrashSelectionActions(
                        selectedCount = selectedIds.size,
                        enabled = !working,
                        restore = { scope.launch { restoreSelected() } },
                        delete = { confirmAction = TrashAction.DELETE_SELECTED },
                        modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
                    )
                } else if (!selecting) {
                    MomentoFloatingButton(
                        modifier = Modifier.align(Alignment.BottomEnd).padding(12.dp),
                        onClick = { confirmAction = TrashAction.EMPTY },
                    ) {
                        Icon(Icons.Default.DeleteForever, "Empty Trash")
                    }
                }
            }
        }
    }

    confirmAction?.let { action ->
        AlertDialog(
            onDismissRequest = { if (!working) confirmAction = null },
            title = { Text(if (action == TrashAction.EMPTY) "Empty Trash?" else "Delete selected media?") },
            text = { Text("This permanently deletes the media and cannot be undone.") },
            confirmButton = {
                TextButton(
                    onClick = { scope.launch { perform(action) } },
                    enabled = !working,
                ) { Text("Delete forever") }
            },
            dismissButton = {
                TextButton(onClick = { confirmAction = null }, enabled = !working) { Text("Cancel") }
            },
        )
    }
    if (current != null && error != null) {
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text("Trash unavailable") },
            text = { Text(error!!) },
            confirmButton = { TextButton(onClick = { error = null }) { Text("OK") } },
        )
    }
}

@Composable
private fun TrashSelectionControl(
    selecting: Boolean,
    selectedCount: Int,
    cancel: () -> Unit,
    start: () -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    Surface(
        modifier = modifier.windowInsetsPadding(WindowInsets.statusBars).padding(top = 10.dp),
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
        shadowElevation = 3.dp,
    ) {
        TextButton(
            onClick = if (selecting) cancel else start,
            colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
        ) { Text(if (selecting) "Cancel ($selectedCount)" else "Select") }
    }
}

@Composable
private fun TrashSelectionActions(
    selectedCount: Int,
    enabled: Boolean,
    restore: () -> Unit,
    delete: () -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    Surface(
        modifier = modifier,
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
        shadowElevation = 5.dp,
    ) {
        Row(Modifier.padding(horizontal = 4.dp)) {
            TextButton(
                onClick = restore,
                enabled = enabled,
                colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
            ) { Text("Restore ($selectedCount)") }
            TextButton(
                onClick = delete,
                enabled = enabled,
                colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
            ) { Text("Delete forever") }
        }
    }
}

fun TrashMedia.asMedia(): Media = Media(
    id = id,
    filename = filename,
    originalFilename = originalFilename,
    mediaType = mediaType,
    mimeType = mimeType,
    width = width,
    height = height,
    fileSize = fileSize,
    durationSeconds = durationSeconds,
    dateTaken = dateTaken,
    gpsLatitude = null,
    gpsLongitude = null,
    locationCity = null,
    locationState = null,
    locationCountry = null,
    createdAt = createdAt,
)
