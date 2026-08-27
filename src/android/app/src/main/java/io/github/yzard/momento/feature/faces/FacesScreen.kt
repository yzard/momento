package io.github.yzard.momento.feature.faces

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Face
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.shouldLoadMoreMedia
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.filter
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

fun canMergeFaceGroups(selectedIds: Set<Long>, working: Boolean): Boolean = selectedIds.size >= 2 && !working

@Composable
fun FacesScreen(
    repository: MomentoRepository,
    isAdmin: Boolean,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var groups by remember(repository) { mutableStateOf<List<FaceGroup>?>(null) }
    var nextCursor by remember(repository) { mutableStateOf<String?>(null) }
    var hasMore by remember(repository) { mutableStateOf(false) }
    var selectedIds by remember(repository) { mutableStateOf<Set<Long>>(emptySet()) }
    var detailGroupId by rememberSaveable { mutableStateOf<Long?>(null) }
    var loading by remember(repository) { mutableStateOf(false) }
    var working by remember(repository) { mutableStateOf(false) }
    var confirmMerge by remember(repository) { mutableStateOf(false) }
    var error by remember(repository) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    suspend fun loadGroups(reset: Boolean) {
        if (loading || (!reset && (!hasMore || nextCursor == null))) return
        loading = true
        if (reset) {
            groups = null
            nextCursor = null
            hasMore = false
        }
        try {
            val response = repository.faces(if (reset) null else nextCursor)
            groups = if (reset) response.groups else appendFaceGroups(groups.orEmpty(), response.groups)
            nextCursor = response.nextCursor
            hasMore = response.hasMore
            error = null
        } catch (_: IOException) {
            error = "Could not load faces"
        } catch (_: HttpException) {
            error = "Could not load faces"
        } catch (_: SerializationException) {
            error = "Could not load faces"
        } finally {
            loading = false
        }
    }

    suspend fun mergeSelected() {
        if (selectedIds.size < 2 || working) return
        working = true
        try {
            repository.mergeFaces(selectedIds.toList())
            selectedIds = emptySet()
            loadGroups(true)
        } catch (_: IOException) {
            error = "Could not merge faces"
        } catch (_: HttpException) {
            error = "Could not merge faces"
        } catch (_: SerializationException) {
            error = "Could not merge faces"
        } finally {
            working = false
        }
    }

    LaunchedEffect(repository) { loadGroups(true) }
    BackHandler(enabled = detailGroupId != null) { detailGroupId = null }

    val selectedGroup = groups?.firstOrNull { group -> group.faceGroupId == detailGroupId }
    if (selectedGroup != null) {
        FaceGroupDetailScreen(repository, selectedGroup, { detailGroupId = null }, openMedia)
        return
    }

    when {
        groups == null && error != null -> ErrorState(error!!) { scope.launch { loadGroups(true) } }
        groups == null -> LoadingState()
        groups!!.isEmpty() -> EmptyState("No faces yet")
        else -> BoxWithConstraints(Modifier.fillMaxSize()) {
            val columns = adaptiveGridColumns(maxWidth.value.toInt())
            val gridState = rememberLazyGridState()
            LaunchedEffect(gridState, hasMore, loading) {
                snapshotFlow {
                    val layout = gridState.layoutInfo
                    shouldLoadMoreMedia(
                        lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                        totalItemsCount = layout.totalItemsCount,
                        hasMore = hasMore,
                        loading = loading,
                    )
                }.filter { it }.collect { loadGroups(false) }
            }
            LazyVerticalGrid(
                columns = GridCells.Fixed(columns),
                state = gridState,
                contentPadding = PaddingValues(top = 64.dp, start = 10.dp, end = 10.dp, bottom = 160.dp),
            ) {
                if (error != null) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Text(requireNotNull(error), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp))
                    }
                }
                items(requireNotNull(groups), key = { it.faceGroupId }) { group ->
                    FaceCard(
                        group = group,
                        repository = repository,
                        selected = group.faceGroupId in selectedIds,
                        selectable = isAdmin,
                        open = { detailGroupId = group.faceGroupId },
                        toggleSelection = {
                            selectedIds = if (group.faceGroupId in selectedIds) {
                                selectedIds - group.faceGroupId
                            } else {
                                selectedIds + group.faceGroupId
                            }
                        },
                    )
                }
                if (hasMore) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                            TextButton(
                                onClick = { scope.launch { loadGroups(false) } },
                                enabled = !loading,
                            ) { Text(if (loading) "Loading more..." else "Load more people") }
                        }
                    }
                }
            }
            if (isAdmin && selectedIds.isNotEmpty()) {
                Surface(
                    modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(12.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    shape = RoundedCornerShape(18.dp),
                    shadowElevation = 8.dp,
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("${selectedIds.size} people selected", fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold, modifier = Modifier.weight(1f))
                        TextButton(onClick = { selectedIds = emptySet() }, enabled = !working) { Text("Clear") }
                        Button(onClick = { confirmMerge = true }, enabled = canMergeFaceGroups(selectedIds, working)) {
                            Text(if (working) "Merging" else "Merge")
                        }
                    }
                }
            }
        }
    }

    if (confirmMerge) {
        AlertDialog(
            onDismissRequest = { if (!working) confirmMerge = false },
            title = { Text("Merge ${selectedIds.size} people?") },
            text = { Text("Their face groups will become one person. Media files are not changed.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmMerge = false
                    scope.launch { mergeSelected() }
                }, enabled = !working) { Text("Merge") }
            },
            dismissButton = { TextButton(onClick = { confirmMerge = false }, enabled = !working) { Text("Cancel") } },
        )
    }
}

@Composable
private fun FaceGroupDetailScreen(
    repository: MomentoRepository,
    group: FaceGroup,
    close: () -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var media by remember(repository, group.faceGroupId) { mutableStateOf<List<Media>?>(null) }
    var error by remember(repository, group.faceGroupId) { mutableStateOf<String?>(null) }
    var retryVersion by remember(repository, group.faceGroupId) { mutableStateOf(0) }

    LaunchedEffect(repository, group.faceGroupId, retryVersion) {
        try {
            media = repository.faceGroup(group.faceGroupId).media
            error = null
        } catch (_: IOException) {
            error = "Could not load this face group"
        } catch (_: HttpException) {
            error = "Could not load this face group"
        } catch (_: SerializationException) {
            error = "Could not load this face group"
        }
    }

    when {
        media == null && error != null -> ErrorState(error!!) { retryVersion += 1 }
        media == null -> LoadingState()
        else -> MediaGrid(
            media = media!!,
            repository = repository,
            selectedMediaIds = emptySet(),
            contentPadding = PaddingValues(top = 64.dp, bottom = 88.dp),
            headerContent = {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = close) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back to people")
                    }
                    Column(Modifier.padding(start = 4.dp)) {
                        Text("Person ${group.faceGroupId}", style = MaterialTheme.typography.titleLarge)
                        Text(
                            "${group.mediaCount} media · ${group.faceCount} faces",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            },
            footerContent = null,
            modifier = Modifier.fillMaxSize(),
        ) { mediaItem ->
            openMedia(media!!, media!!.indexOf(mediaItem))
        }
    }
}

@Composable
private fun FaceCard(
    group: FaceGroup,
    repository: MomentoRepository,
    selected: Boolean,
    selectable: Boolean,
    open: () -> Unit,
    toggleSelection: () -> Unit,
) {
    var image by remember(group.faceGroupId) { mutableStateOf<ByteArray?>(null) }
    LaunchedEffect(repository, group.faceGroupId) {
        image = try {
            repository.faceThumbnail(group.faceGroupId)
        } catch (_: IOException) {
            null
        } catch (_: HttpException) {
            null
        } catch (_: SerializationException) {
            null
        }
    }
    Card(
        modifier = Modifier.padding(6.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerLow,
        ),
    ) {
        Box {
            if (image == null) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .aspectRatio(1f)
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .clickable(onClick = open),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Default.Face, "No face thumbnail") }
            } else {
                AsyncImage(
                    model = image,
                    contentDescription = "Person ${group.faceGroupId}",
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxWidth().aspectRatio(1f).clickable(onClick = open),
                )
            }
            if (selectable) {
                IconButton(onClick = toggleSelection, modifier = Modifier.align(Alignment.TopEnd)) {
                    Icon(
                        if (selected) Icons.Default.CheckCircle else Icons.Default.RadioButtonUnchecked,
                        if (selected) "Deselect person ${group.faceGroupId}" else "Select person ${group.faceGroupId}",
                        tint = if (selected) MaterialTheme.colorScheme.primary else Color.White,
                    )
                }
            }
        }
        Column(Modifier.fillMaxWidth().clickable(onClick = open).padding(12.dp)) {
            Text("Person ${group.faceGroupId}", style = MaterialTheme.typography.titleSmall)
            Text("${group.mediaCount} media · ${group.faceCount} faces", style = MaterialTheme.typography.bodySmall)
        }
    }
}

fun appendFaceGroups(existing: List<FaceGroup>, page: List<FaceGroup>): List<FaceGroup> =
    existing + page.filter { candidate -> existing.none { it.faceGroupId == candidate.faceGroupId } }
