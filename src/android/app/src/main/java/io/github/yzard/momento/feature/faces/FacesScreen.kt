package io.github.yzard.momento.feature.faces

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
import androidx.compose.material.icons.automirrored.filled.CallMerge
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Face
import androidx.compose.material.icons.filled.RadioButtonUnchecked
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
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.RequestResult
import io.github.yzard.momento.core.data.runRequest
import io.github.yzard.momento.core.data.userMessage
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.ui.MomentoAsyncImage
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.app.designsystem.MomentoSelectionDock
import io.github.yzard.momento.app.designsystem.MomentoSelectionMark
import io.github.yzard.momento.app.navigation.LibraryChange
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import io.github.yzard.momento.feature.media.MomentoCollectionDetail
import io.github.yzard.momento.feature.media.PageState
import io.github.yzard.momento.feature.media.asReadyPage
import io.github.yzard.momento.feature.media.beginCursorPage
import io.github.yzard.momento.feature.media.completeCursorPage
import io.github.yzard.momento.feature.media.emptyCursorPagingState
import io.github.yzard.momento.feature.media.failCursorPage
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.shouldLoadMoreMedia
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.filter

fun canMergeFaceGroups(selectedIds: Set<Long>, working: Boolean): Boolean = selectedIds.size >= 2 && !working

@Composable
fun FacesScreen(
    repository: MomentoRepository,
    isAdmin: Boolean,
    libraryChange: LibraryChange?,
    openFace: (FaceGroup) -> Unit,
) {
    var pagingState by remember(repository) { mutableStateOf(emptyCursorPagingState<FaceGroup>()) }
    var selectedIds by remember(repository) { mutableStateOf<Set<Long>>(emptySet()) }
    var working by remember(repository) { mutableStateOf(false) }
    var confirmMerge by remember(repository) { mutableStateOf(false) }
    var actionError by remember(repository) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    suspend fun loadGroups(reset: Boolean) {
        val loadingState = beginCursorPage(pagingState, reset) ?: return
        pagingState = loadingState
        when (val requestResult = runRequest { repository.faces(if (reset) null else loadingState.nextCursor) }) {
            is RequestResult.Success -> pagingState = completeCursorPage(
                state = loadingState,
                page = requestResult.response.groups,
                nextCursor = requestResult.response.nextCursor,
                hasMore = requestResult.response.hasMore,
                key = FaceGroup::faceGroupId,
            )
            is RequestResult.Failure -> pagingState = failCursorPage(
                loadingState,
                requestResult.error.userMessage("Could not load people"),
            )
        }
    }

    suspend fun mergeSelected() {
        if (selectedIds.size < 2 || working) return
        working = true
        when (val requestResult = runRequest { repository.mergeFaces(selectedIds.toList()) }) {
            is RequestResult.Success -> {
                selectedIds = emptySet()
                loadGroups(true)
            }
            is RequestResult.Failure -> {
                actionError = requestResult.error.userMessage("Could not merge people")
            }
        }
        working = false
    }

    LaunchedEffect(repository, libraryChange?.sequence) { loadGroups(true) }
    MomentoPageScaffold(
        title = "Faces",
        subtitle = null,
        backContentDescription = null,
        onBack = null,
        trailingContent = null,
        reserveBottomControls = true,
        edgeToEdgeContent = false,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
    when {
        !pagingState.initialized && pagingState.error != null -> ErrorState(
            requireNotNull(pagingState.error),
            { scope.launch { loadGroups(true) } },
            Modifier,
        )
        !pagingState.initialized -> LoadingState("Loading people", Modifier)
        pagingState.entries.isEmpty() -> EmptyState(
            "No people yet",
            "Detected people will appear here after face analysis completes.",
            Modifier,
        )
        else -> BoxWithConstraints(Modifier.fillMaxSize()) {
            val columns = adaptiveGridColumns(maxWidth.value.toInt())
            val gridState = rememberLazyGridState()
            LaunchedEffect(gridState, pagingState.hasMore, pagingState.loading) {
                snapshotFlow {
                    val layout = gridState.layoutInfo
                    shouldLoadMoreMedia(
                        lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                        totalItemsCount = layout.totalItemsCount,
                        hasMore = pagingState.hasMore,
                        loading = pagingState.loading,
                    )
                }.filter { it }.collect { loadGroups(false) }
            }
            LazyVerticalGrid(
                columns = GridCells.Fixed(columns),
                state = gridState,
                contentPadding = contentPadding,
            ) {
                if (pagingState.error != null) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Text(requireNotNull(pagingState.error), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp))
                    }
                }
                items(pagingState.entries, key = { it.faceGroupId }) { group ->
                    FaceCard(
                        group = group,
                        repository = repository,
                        selected = group.faceGroupId in selectedIds,
                        selectable = isAdmin,
                        open = { openFace(group) },
                        toggleSelection = {
                            selectedIds = if (group.faceGroupId in selectedIds) {
                                selectedIds - group.faceGroupId
                            } else {
                                selectedIds + group.faceGroupId
                            }
                        },
                    )
                }
                if (pagingState.hasMore) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                            TextButton(
                                onClick = { scope.launch { loadGroups(false) } },
                                enabled = !pagingState.loading,
                            ) { Text(if (pagingState.loading) "Loading more..." else "Load more people") }
                        }
                    }
                }
            }
            if (isAdmin && selectedIds.isNotEmpty()) {
                MomentoSelectionDock(
                    selectedCount = selectedIds.size,
                    actions = listOf(
                        MomentoSelectionAction(
                            label = if (working) "Merging" else "Merge",
                            icon = Icons.AutoMirrored.Filled.CallMerge,
                            enabled = canMergeFaceGroups(selectedIds, working),
                            destructive = false,
                            perform = { confirmMerge = true },
                        ),
                    ),
                    clearSelection = { selectedIds = emptySet() },
                    modifier = Modifier.align(Alignment.BottomCenter).padding(12.dp),
                )
            }
        }
    }
    }

    if (actionError != null) {
        AlertDialog(
            onDismissRequest = { actionError = null },
            title = { Text("People unavailable") },
            text = { Text(requireNotNull(actionError)) },
            confirmButton = { TextButton(onClick = { actionError = null }) { Text("OK") } },
        )
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
internal fun FaceGroupDetailScreen(
    repository: MomentoRepository,
    group: FaceGroup,
    close: () -> Unit,
    libraryChange: LibraryChange?,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var pageState by remember(repository, group.faceGroupId) {
        mutableStateOf<PageState<List<Media>>>(PageState.Loading)
    }
    var retryVersion by remember(repository, group.faceGroupId) { mutableStateOf(0) }

    LaunchedEffect(repository, group.faceGroupId, retryVersion, libraryChange?.sequence) {
        when (val requestResult = runRequest { repository.faceGroup(group.faceGroupId) }) {
            is RequestResult.Success -> {
                pageState = requestResult.response.media.asReadyPage()
            }
            is RequestResult.Failure -> {
                pageState = PageState.Failed(
                    requestResult.error.userMessage("Could not load this face group"),
                )
            }
        }
    }

    MomentoCollectionDetail(
        title = "Person ${group.faceGroupId}",
        subtitle = "${group.mediaCount} media · ${group.faceCount} faces",
        backContentDescription = "Back to people",
        repository = repository,
        pageState = pageState,
        selectedMediaIds = emptySet(),
        reserveBottomControls = false,
        bottomContent = null,
        footerContent = null,
        contentError = null,
        loadingLabel = "Loading person",
        emptyTitle = "No media",
        emptyExplanation = "This person has no visible media.",
        close = close,
        retry = { retryVersion += 1 },
        select = { mediaItem, media -> openMedia(media, media.indexOf(mediaItem)) },
    )
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
        image = when (val requestResult = runRequest { repository.faceThumbnail(group.faceGroupId) }) {
            is RequestResult.Success -> requestResult.response
            is RequestResult.Failure -> null
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
                MomentoAsyncImage(
                    model = image,
                    repository = repository,
                    contentDescription = "Person ${group.faceGroupId}",
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxWidth().aspectRatio(1f).clickable(onClick = open),
                )
            }
            if (selectable) {
                IconButton(onClick = toggleSelection, modifier = Modifier.align(Alignment.TopEnd)) {
                    MomentoSelectionMark(
                        selected = selected,
                        contentDescription = if (selected) {
                            "Deselect person ${group.faceGroupId}"
                        } else {
                            "Select person ${group.faceGroupId}"
                        },
                        modifier = Modifier,
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
