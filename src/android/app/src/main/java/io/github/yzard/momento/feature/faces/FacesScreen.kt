package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.collectAsState
import io.github.yzard.momento.core.model.FaceDetection
import io.github.yzard.momento.core.model.RejectFacesRequest
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
import kotlinx.coroutines.launch

fun canMergeFaceGroups(selectedIds: Set<Long>, working: Boolean): Boolean = selectedIds.size >= 2 && !working

@Composable
fun FacesScreen(
    repository: MomentoRepository,
    isAdmin: Boolean,
    libraryChange: LibraryChange?,
    active: Boolean,
    openFace: (FaceGroup) -> Unit,
) {
    var pagingState by remember(repository) { mutableStateOf(emptyCursorPagingState<FaceGroup>()) }
    var selectedIds by remember(repository) { mutableStateOf<Set<Long>>(emptySet()) }
    val rejectionId = remember(selectedIds) { java.util.UUID.randomUUID().toString() }
    var working by remember(repository) { mutableStateOf(false) }
    var confirmReject by remember { mutableStateOf(false) }
    val faceRevision by repository.faceRevision.collectAsState()
    var confirmMerge by remember(repository) { mutableStateOf(false) }
    var actionError by remember(repository) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val gridState = rememberLazyGridState()

    val requestGeneration = remember(repository) { longArrayOf(0) }
    suspend fun loadGroups(reset: Boolean) {
        if (reset) { requestGeneration[0] = requestGeneration[0] + 1L; pagingState = pagingState.copy(loading = false) }
        val generation = requestGeneration[0]
        if (reset && pagingState.initialized && !pagingState.loading) {
            val previous = pagingState
            val anchorIndex = gridState.firstVisibleItemIndex
            val anchor = previous.entries.getOrNull(anchorIndex)?.faceGroupId
            val offset = gridState.firstVisibleItemScrollOffset
            pagingState = previous.copy(loading = true)
            var refreshed = emptyCursorPagingState<FaceGroup>()
            do {
                val result = runRequest { repository.faces(refreshed.nextCursor) }
                if (generation != requestGeneration[0]) return
                when (result) {
                    is RequestResult.Success -> refreshed = completeCursorPage(refreshed, result.response.groups, result.response.nextCursor, result.response.hasMore, FaceGroup::faceGroupId)
                    is RequestResult.Failure -> { pagingState = previous.copy(loading = false, error = result.error.userMessage("Could not reload faces")); return }
                }
            } while (refreshed.hasMore && refreshed.entries.size < previous.entries.size)
            pagingState = refreshed
            selectedIds = selectedIds.intersect(refreshed.entries.map { it.faceGroupId }.toSet())
            if (refreshed.entries.isNotEmpty()) gridState.scrollToItem(refreshed.entries.indexOfFirst { it.faceGroupId == anchor }.takeIf { it >= 0 } ?: anchorIndex.coerceAtMost(refreshed.entries.lastIndex), offset)
            return
        }
        val loadingState = beginCursorPage(pagingState, reset) ?: return
        pagingState = loadingState
        val requestResult = runRequest { repository.faces(if (reset) null else loadingState.nextCursor) }
        if (generation != requestGeneration[0]) return
        when (requestResult) {
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

    LaunchedEffect(repository, libraryChange?.sequence, faceRevision) { loadGroups(true) }
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
            if (active) FaceGroupPagingEffect(gridState, pagingState) { loadGroups(false) }
            LazyVerticalGrid(
                columns = GridCells.Fixed(columns),
                state = gridState,
                contentPadding = PaddingValues(
                    start = contentPadding.calculateLeftPadding(androidx.compose.ui.unit.LayoutDirection.Ltr),
                    end = contentPadding.calculateRightPadding(androidx.compose.ui.unit.LayoutDirection.Ltr),
                    top = contentPadding.calculateTopPadding(),
                    bottom = contentPadding.calculateBottomPadding() + if (isAdmin && selectedIds.isNotEmpty()) 176.dp else 0.dp,
                ),
            ) {
                if (pagingState.error != null) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Text(requireNotNull(pagingState.error), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp))
                    }
                }
                items(pagingState.entries, key = { it.faceGroupId }) { group ->
                    FaceCard(
                        revision = faceRevision,
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
                FaceSelectionPanel(
                    selectedIds = selectedIds.toList(),
                    working = working,
                    actions = listOf(
                        MomentoSelectionAction("Not a face", Icons.Default.Face, !working, true, { confirmReject = true }),
                        MomentoSelectionAction(
                            label = if (working) "Merging" else "Merge",
                            icon = Icons.AutoMirrored.Filled.CallMerge,
                            enabled = canMergeFaceGroups(selectedIds, working),
                            destructive = false,
                            perform = { confirmMerge = true },
                        ),
                    ),
                    removeSelection = { id -> selectedIds = selectedIds - id },
                    clearSelection = { selectedIds = emptySet() },
                    thumbnail = { id, modifier -> SmallFaceThumbnail(repository, id, true, modifier) },
                    modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding()
                        .padding(horizontal = 16.dp)
                        .padding(bottom = if (maxWidth < 600.dp) 84.dp else 12.dp),
                )
            }
        }
    }
    }

    if (confirmReject && isAdmin) AlertDialog(
        onDismissRequest = { if (!working) confirmReject = false },
        title = { Text("Not a face?") },
        text = { Text("Exclude the selected groups for everyone. Media will be kept. Only Clean AI Face Data can reset exclusions.") },
        confirmButton = { TextButton(enabled = !working, onClick = {
            working = true
            scope.launch {
                when (val result = runRequest { repository.rejectFaces(RejectFacesRequest(rejectionId, selectedIds.toList(), null, emptyList())) }) {
                    is RequestResult.Success -> { selectedIds = emptySet(); confirmReject = false; working = false; loadGroups(true) }
                    is RequestResult.Failure -> { actionError = result.error.userMessage("Could not exclude faces"); working = false }
                }
            }
        }) { Text("Not a face") } },
        dismissButton = { TextButton(enabled = !working, onClick = { confirmReject = false }) { Text("Cancel") } },
    )
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
    isAdmin: Boolean,
    close: () -> Unit,
    libraryChange: LibraryChange?,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var detections by remember(group.faceGroupId) { mutableStateOf<List<FaceDetection>>(emptyList()) }
    var selectedMedia by remember(group.faceGroupId) { mutableStateOf<Set<Long>>(emptySet()) }
    var selectedFaces by remember(group.faceGroupId) { mutableStateOf<Set<Long>>(emptySet()) }
    val rejectionId = remember(selectedFaces) { java.util.UUID.randomUUID().toString() }
    var selecting by remember { mutableStateOf(false) }
    var working by remember { mutableStateOf(false) }
    var confirmReject by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    var pageState by remember(repository, group.faceGroupId) {
        mutableStateOf<PageState<List<Media>>>(PageState.Loading)
    }
    var retryVersion by remember(repository, group.faceGroupId) { mutableStateOf(0) }

    LaunchedEffect(repository, group.faceGroupId, retryVersion, libraryChange?.sequence) {
        when (val requestResult = runRequest { repository.faceGroup(group.faceGroupId) }) {
            is RequestResult.Success -> {
                pageState = requestResult.response.media.asReadyPage()
                detections = requestResult.response.faces
            }
            is RequestResult.Failure -> {
                pageState = PageState.Failed(
                    requestResult.error.userMessage("Could not load this face group"),
                )
            }
        }
    }

    val displayedMediaCount = when (val state = pageState) { is PageState.Ready -> state.content.size.toLong(); else -> group.mediaCount }
    MomentoCollectionDetail(
        title = "Face Group #${group.faceGroupId}",
        subtitle = "$displayedMediaCount media",
        backContentDescription = "Back to people",
        repository = repository,
        pageState = pageState,
        selectedMediaIds = selectedMedia,
        reserveBottomControls = isAdmin,
        bottomContent = if (!isAdmin) null else { {
            Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(MaterialTheme.colorScheme.surface).padding(12.dp)) {
                Row {
                    TextButton(onClick = { selecting = !selecting; selectedMedia = emptySet(); selectedFaces = emptySet() }) { Text(if (selecting) "Clear" else "Select media") }
                    TextButton(enabled = selectedFaces.isNotEmpty() && !working, onClick = { confirmReject = true }) { Text("Not a face (${selectedFaces.size})") }
                }
                if (selectedMedia.isNotEmpty()) {
                    Text("Choose exact faces. Other faces in the media are kept.")
                    LazyRow { items(detections.filter { it.mediaId in selectedMedia }, key = { it.faceId }) { face ->
                        Column(Modifier.clickable(enabled = !working) { selectedFaces = if (face.faceId in selectedFaces) selectedFaces - face.faceId else selectedFaces + face.faceId }) {
                            SmallFaceThumbnail(repository, face.faceId, false, Modifier.size(64.dp))
                            Text("${if (face.faceId in selectedFaces) "✓ " else ""}Face #${face.faceId}")
                        }
                    } }
                }
            }
        } },
        footerContent = null,
        contentError = error,
        loadingLabel = "Loading person",
        emptyTitle = "No media",
        emptyExplanation = "This person has no visible media.",
        close = close,
        retry = { retryVersion += 1 },
        select = { mediaItem, media ->
            if (!selecting || !isAdmin) openMedia(media, media.indexOf(mediaItem))
            else {
                val adding = mediaItem.id !in selectedMedia
                selectedMedia = if (adding) selectedMedia + mediaItem.id else selectedMedia - mediaItem.id
                val candidates = detections.filter { it.mediaId == mediaItem.id }
                selectedFaces = toggleMediaFaceSelection(selectedFaces, candidates, adding)
            }
        },
    )
    if (confirmReject && isAdmin) AlertDialog(
        onDismissRequest = { if (!working) confirmReject = false }, title = { Text("Not a face?") },
        text = { Text("Exclude only these detections for everyone. Media and other faces will be kept.") },
        confirmButton = { TextButton(enabled = !working, onClick = {
            working = true
            scope.launch {
                when (val result = runRequest { repository.rejectFaces(RejectFacesRequest(rejectionId, emptyList(), group.faceGroupId, selectedFaces.toList())) }) {
                    is RequestResult.Success -> { selectedFaces = emptySet(); selectedMedia = emptySet(); confirmReject = false
                        when (val refreshed = runRequest { repository.faceGroup(group.faceGroupId) }) {
                            is RequestResult.Success -> { pageState = refreshed.response.media.asReadyPage(); detections = refreshed.response.faces; if (refreshed.response.media.isEmpty()) close() }
                            is RequestResult.Failure -> close()
                        }
                    }
                    is RequestResult.Failure -> error = result.error.userMessage("Could not exclude faces")
                }
                working = false
            }
        }) { Text("Not a face") } },
        dismissButton = { TextButton(enabled = !working, onClick = { confirmReject = false }) { Text("Cancel") } },
    )

}

@Composable
private fun FaceCard(
    revision: Int,
    group: FaceGroup,
    repository: MomentoRepository,
    selected: Boolean,
    selectable: Boolean,
    open: () -> Unit,
    toggleSelection: () -> Unit,
) {
    var image by remember(group.faceGroupId) { mutableStateOf<ByteArray?>(null) }
    LaunchedEffect(repository, group.faceGroupId, revision) {
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
        Box(Modifier.clickable(onClick = open)) {
            if (image == null) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .aspectRatio(1f)
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Default.Face, "No face thumbnail") }
            } else {
                MomentoAsyncImage(
                    model = image,
                    repository = repository,
                    contentDescription = "Person ${group.faceGroupId}",
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxWidth().aspectRatio(1f),
                )
            }
            if (selectable) {
                IconButton(onClick = toggleSelection, modifier = Modifier.align(Alignment.TopStart)) {
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
            Text(
                text = group.mediaCount.toString(),
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(8.dp)
                    .background(Color.Black.copy(alpha = 0.65f), RoundedCornerShape(50))
                    .padding(horizontal = 10.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun SmallFaceThumbnail(repository: MomentoRepository, id: Long, group: Boolean, modifier: Modifier) {
    val revision by repository.faceRevision.collectAsState()
    var bytes by remember(id, group) { mutableStateOf<ByteArray?>(null) }
    LaunchedEffect(repository, id, group, revision) {
        bytes = when(val result = runRequest { if (group) repository.faceThumbnail(id) else repository.faceCrop(id) }) {
            is RequestResult.Success -> result.response
            is RequestResult.Failure -> null
        }
    }
    MomentoAsyncImage(model = bytes, repository = repository, contentDescription = "Face $id", contentScale = ContentScale.Crop, modifier = modifier)
}

internal fun toggleMediaFaceSelection(selected: Set<Long>, candidates: List<FaceDetection>, adding: Boolean): Set<Long> {
    val remaining = selected - candidates.map { it.faceId }.toSet()
    return if (adding && candidates.size == 1) remaining + candidates.single().faceId else remaining
}
