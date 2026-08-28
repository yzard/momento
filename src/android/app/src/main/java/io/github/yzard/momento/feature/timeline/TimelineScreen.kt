package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.AddPhotoAlternate
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.app.designsystem.MomentoFloatingDock
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.app.designsystem.MomentoSelectionDock
import io.github.yzard.momento.app.navigation.LibraryChange
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.RequestResult
import io.github.yzard.momento.core.data.runRequest
import io.github.yzard.momento.core.data.userMessage
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.feature.albums.AlbumAddMediaSheet
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.PageState
import io.github.yzard.momento.feature.media.SelectableMediaThumbnail
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.toggleMediaSelection
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset

enum class TimelinePage(
    val label: String,
    val mediaType: String?,
    val classification: String?,
) {
    TIMELINE("Timeline", null, null),
    PHOTOS("Photos", "image", null),
    VIDEOS("Videos", "video", null),
    SCREENSHOTS("Screenshots", "image", "screenshot"),
    DOCUMENTS("Documents", "image", "document"),
}

enum class TimelinePeriod(val label: String, val groupBy: String) {
    DAY("Day", "day"),
    WEEK("Week", "week"),
    MONTH("Month", "month"),
    YEAR("Year", "year"),
}

data class TimelineMediaItem(val period: String, val media: Media)
data class TimelineScrollPosition(val index: Int, val offset: Int)

private val timelineScrollPositions = mutableMapOf<String, TimelineScrollPosition>()

fun normalizedTimelineSearchQuery(value: String): String = value.trim()

fun timelineScrollKey(page: TimelinePage, period: TimelinePeriod, search: String): String =
    "${page.name}:${period.name}:$search"

fun shouldAppendTimeline(
    lastVisibleItemIndex: Int,
    totalItemsCount: Int,
    hasOlder: Boolean,
    appending: Boolean,
): Boolean = hasOlder && !appending && totalItemsCount > 0 && lastVisibleItemIndex >= totalItemsCount - 3

fun shouldPrependTimeline(
    firstVisibleItemIndex: Int,
    hasNewer: Boolean,
    prepending: Boolean,
    scrollingTowardStart: Boolean,
): Boolean = hasNewer && !prepending && scrollingTowardStart && firstVisibleItemIndex <= 2

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun TimelineScreen(
    repository: MomentoRepository,
    page: TimelinePage,
    period: TimelinePeriod,
    search: String,
    libraryChange: LibraryChange?,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val normalizedSearch = normalizedTimelineSearchQuery(search)
    key(page, period, normalizedSearch) {
        TimelinePageContent(repository, page, period, normalizedSearch, libraryChange, openMedia)
    }
}

@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun TimelinePageContent(
    repository: MomentoRepository,
    page: TimelinePage,
    period: TimelinePeriod,
    search: String,
    libraryChange: LibraryChange?,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var pagingState by remember { mutableStateOf(TimelinePagingState.initial()) }
    var selecting by remember { mutableStateOf(false) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var confirmTrash by remember { mutableStateOf(false) }
    var addingToAlbum by remember { mutableStateOf(false) }
    var selectionError by remember { mutableStateOf<String?>(null) }
    var showPeriodPicker by remember { mutableStateOf(false) }
    var pickerInitialDate by remember { mutableStateOf(LocalDate.now(ZoneOffset.UTC)) }
    var anchorDate by remember { mutableStateOf(Instant.now().toString()) }
    var refreshVersion by remember { mutableStateOf(0) }
    val scrollKey = remember(page, period, search) { timelineScrollKey(page, period, search) }
    val restoredPosition = remember(scrollKey) { timelineScrollPositions[scrollKey] ?: TimelineScrollPosition(0, 0) }
    val gridState = rememberLazyGridState(restoredPosition.index, restoredPosition.offset)
    val scope = rememberCoroutineScope()

    suspend fun load(reset: Boolean, direction: TimelineDirection) {
        val requestCursor = pagingState.cursor(direction)
        val loadingState = pagingState.begin(reset, direction) ?: return
        pagingState = loadingState
        when (val requestResult = runRequest {
            repository.timelinePage(
                cursor = if (reset) null else requestCursor,
                groupBy = period.groupBy,
                search = search,
                mediaType = page.mediaType,
                classification = page.classification,
                direction = direction.wireValue,
                anchorDate = anchorDate,
            )
        }) {
            is RequestResult.Success -> {
                pagingState = loadingState.complete(requestResult.response, reset, direction)
            }
            is RequestResult.Failure -> {
                pagingState = loadingState.fail(
                    direction,
                    requestResult.error.userMessage("Could not load timeline"),
                )
            }
        }
    }

    suspend fun loadNewer() {
        val firstIndex = gridState.firstVisibleItemIndex
        val firstOffset = gridState.firstVisibleItemScrollOffset
        val existingGroups = (pagingState.page as? PageState.Ready)?.content.orEmpty()
        val existingIds = flattenTimelineGroups(existingGroups).mapTo(mutableSetOf()) { it.media.id }
        load(reset = false, direction = TimelineDirection.NEWER)
        val updatedGroups = (pagingState.page as? PageState.Ready)?.content.orEmpty()
        val addedCount = flattenTimelineGroups(updatedGroups).count { it.media.id !in existingIds }
        if (addedCount > 0) gridState.scrollToItem(firstIndex + addedCount, firstOffset)
    }

    suspend fun moveSelectedToTrash() {
        if (selectedIds.isEmpty()) return
        confirmTrash = false
        when (val requestResult = runRequest { repository.moveToTrash(selectedIds.toList()) }) {
            is RequestResult.Success -> {
                val groups = (pagingState.page as? PageState.Ready)?.content.orEmpty()
                pagingState = pagingState.copy(
                    page = PageState.Ready(
                        removeTimelineMedia(groups, selectedIds),
                        refreshing = false,
                    ),
                )
                selectedIds = emptySet()
                selecting = false
            }
            is RequestResult.Failure -> {
                selectionError = requestResult.error.userMessage("Could not move the selected media to Trash")
            }
        }
    }

    fun refreshTimeline() {
        anchorDate = Instant.now().toString()
        refreshVersion += 1
    }

    LaunchedEffect(repository, page, period, search, anchorDate, refreshVersion, libraryChange?.sequence) {
        load(reset = true, direction = TimelineDirection.OLDER)
    }

    LaunchedEffect(gridState, pagingState.olderCursor, pagingState.hasOlder) {
        snapshotFlow {
            val layout = gridState.layoutInfo
            shouldAppendTimeline(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasOlder = pagingState.hasOlder && pagingState.message == null,
                appending = pagingState.loadingDirection != null,
            )
        }.filter { it }.collect { load(reset = false, direction = TimelineDirection.OLDER) }
    }

    LaunchedEffect(gridState, pagingState.newerCursor, pagingState.hasNewer) {
        snapshotFlow {
            shouldPrependTimeline(
                firstVisibleItemIndex = gridState.firstVisibleItemIndex,
                hasNewer = pagingState.hasNewer && pagingState.message == null,
                prepending = pagingState.loadingDirection != null,
                scrollingTowardStart = gridState.isScrollInProgress && gridState.lastScrolledBackward,
            )
        }.filter { it }.collect { loadNewer() }
    }

    DisposableEffect(scrollKey, gridState) {
        onDispose {
            timelineScrollPositions[scrollKey] = TimelineScrollPosition(
                index = gridState.firstVisibleItemIndex,
                offset = gridState.firstVisibleItemScrollOffset,
            )
        }
    }

    MomentoPageScaffold(
        title = page.label,
        subtitle = null,
        backContentDescription = null,
        onBack = null,
        trailingContent = null,
        reserveBottomControls = true,
        edgeToEdgeContent = true,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
    when (val pageState = pagingState.page) {
        is PageState.Failed -> ErrorState(
            pageState.message,
            { scope.launch { load(reset = true, direction = TimelineDirection.OLDER) } },
            Modifier,
        )
        PageState.Loading -> LoadingState("Loading timeline", Modifier)
        is PageState.Ready -> PullToRefreshBox(
            isRefreshing = pageState.refreshing,
            onRefresh = ::refreshTimeline,
            modifier = Modifier.fillMaxSize(),
        ) {
            ContinuousTimelineGrid(
                groups = pageState.content,
                repository = repository,
                gridState = gridState,
                loadingOlder = pagingState.loadingDirection == TimelineDirection.OLDER,
                loadingNewer = pagingState.loadingDirection == TimelineDirection.NEWER,
                error = pagingState.message,
                failedDirection = pagingState.failedDirection?.wireValue,
                selecting = selecting,
                selectedIds = selectedIds,
                contentPadding = contentPadding,
                startSelecting = { selecting = true },
                cancelSelecting = {
                    selecting = false
                    selectedIds = emptySet()
                },
                toggleSelection = { mediaId -> selectedIds = toggleMediaSelection(selectedIds, mediaId) },
                requestTrash = { confirmTrash = true },
                requestAddToAlbum = { addingToAlbum = true },
                retryOlder = { scope.launch { load(reset = false, direction = TimelineDirection.OLDER) } },
                retryNewer = { scope.launch { loadNewer() } },
                choosePeriod = { visiblePeriod ->
                    pickerInitialDate = timelinePeriodInitialDate(
                        period = period,
                        label = visiblePeriod,
                        fallback = LocalDate.now(ZoneOffset.UTC),
                    )
                    showPeriodPicker = true
                },
                openMedia = openMedia,
            )
        }
    }
    }

    if (confirmTrash) {
        AlertDialog(
            onDismissRequest = { confirmTrash = false },
            title = { Text("Move to Trash?") },
            text = { Text("Move ${selectedIds.size} selected item${if (selectedIds.size == 1) "" else "s"} to Trash?") },
            confirmButton = { TextButton(onClick = { scope.launch { moveSelectedToTrash() } }) { Text("Move") } },
            dismissButton = { TextButton(onClick = { confirmTrash = false }) { Text("Cancel") } },
        )
    }
    if (addingToAlbum) {
        ModalBottomSheet(onDismissRequest = { addingToAlbum = false }) {
            AlbumAddMediaSheet(
                repository = repository,
                mediaIds = selectedIds.toList(),
                close = {
                    addingToAlbum = false
                    selectedIds = emptySet()
                    selecting = false
                },
            )
        }
    }
    selectionError?.let { message ->
        AlertDialog(
            onDismissRequest = { selectionError = null },
            title = { Text("Trash unavailable") },
            text = { Text(message) },
            confirmButton = { TextButton(onClick = { selectionError = null }) { Text("OK") } },
        )
    }
    if (showPeriodPicker) {
        TimelinePeriodPicker(
            period = period,
            initialDate = pickerInitialDate,
            dismiss = { showPeriodPicker = false },
            select = { selectedDate ->
                pagingState = TimelinePagingState.initial()
                anchorDate = timelinePeriodAnchorDate(period, selectedDate)
                refreshVersion += 1
                showPeriodPicker = false
                scope.launch { gridState.scrollToItem(0) }
            },
        )
    }
}

@Composable
private fun ContinuousTimelineGrid(
    groups: List<TimelineGroup>,
    repository: MomentoRepository,
    gridState: LazyGridState,
    loadingOlder: Boolean,
    loadingNewer: Boolean,
    error: String?,
    failedDirection: String?,
    selecting: Boolean,
    selectedIds: Set<Long>,
    contentPadding: PaddingValues,
    startSelecting: () -> Unit,
    cancelSelecting: () -> Unit,
    toggleSelection: (Long) -> Unit,
    requestTrash: () -> Unit,
    requestAddToAlbum: () -> Unit,
    retryOlder: () -> Unit,
    retryNewer: () -> Unit,
    choosePeriod: (String) -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val timelineMedia = remember(groups) { flattenTimelineGroups(groups) }
    if (timelineMedia.isEmpty()) {
        EmptyState("No media yet", "Imported memories will appear here when they are ready.", Modifier)
        return
    }
    val allMedia = remember(timelineMedia) { timelineMedia.map { it.media } }
    val leadingItemCount = if (failedDirection == "newer") 1 else 0
    val visiblePeriod by remember(timelineMedia, gridState, leadingItemCount) {
        derivedStateOf {
            timelinePeriodAtIndex(timelineMedia, (gridState.firstVisibleItemIndex - leadingItemCount).coerceAtLeast(0))
                ?: timelineMedia.last().period
        }
    }

    BoxWithConstraints(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        val columns = adaptiveGridColumns(maxWidth.value.toInt())
        LazyVerticalGrid(
            columns = GridCells.Fixed(columns),
            state = gridState,
            contentPadding = contentPadding,
        ) {
            if (failedDirection == "newer") {
                item(key = "newer", span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TextButton(
                            onClick = retryNewer,
                            enabled = !loadingNewer,
                        ) {
                            Text("Could not load newer media. Retry")
                        }
                    }
                }
            }
            items(timelineMedia, key = { it.media.id }) { timelineItem ->
                SelectableMediaThumbnail(
                    media = timelineItem.media,
                    repository = repository,
                    trashed = false,
                    selected = timelineItem.media.id in selectedIds,
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(1f)
                        .padding(0.5.dp)
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .clickable {
                            if (selecting) {
                                toggleSelection(timelineItem.media.id)
                            } else {
                                openMedia(allMedia, allMedia.indexOfFirst { it.id == timelineItem.media.id })
                            }
                        },
                )
            }
            if (loadingOlder) {
                item(key = "older-loading", span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth().padding(20.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator()
                    }
                }
            } else if (error != null && failedDirection == "older") {
                item(key = "older-error", span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TextButton(onClick = retryOlder) { Text("Could not load older media. Retry") }
                    }
                }
            }
        }
        FloatingTimelineHeader(
            label = visiblePeriod,
            choosePeriod = { choosePeriod(visiblePeriod) },
            modifier = Modifier.align(Alignment.TopCenter),
        )
        TimelineSelectionControl(
            selecting = selecting,
            startSelecting = startSelecting,
            cancelSelecting = cancelSelecting,
            modifier = Modifier.align(Alignment.TopEnd).padding(end = 12.dp),
        )
        if (selectedIds.isNotEmpty()) {
            MomentoSelectionDock(
                selectedCount = selectedIds.size,
                actions = listOf(
                    MomentoSelectionAction(
                        label = "Add to album",
                        icon = Icons.Default.AddPhotoAlternate,
                        enabled = true,
                        destructive = false,
                        perform = requestAddToAlbum,
                    ),
                    MomentoSelectionAction(
                        label = "Move to Trash",
                        icon = Icons.Default.Delete,
                        enabled = true,
                        destructive = true,
                        perform = requestTrash,
                    ),
                ),
                clearSelection = cancelSelecting,
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
            )
        }
    }
}

@Composable
private fun FloatingTimelineHeader(
    label: String,
    choosePeriod: () -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    Surface(
        modifier = modifier.windowInsetsPadding(WindowInsets.statusBars).padding(top = 10.dp),
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
        shadowElevation = 3.dp,
        tonalElevation = 1.dp,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.clickable(onClick = choosePeriod).padding(horizontal = 14.dp, vertical = 10.dp),
        ) {
            Icon(Icons.Default.CalendarMonth, "Choose period", Modifier.size(18.dp))
            Spacer(Modifier.size(8.dp))
            Text(text = label, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun TimelineSelectionControl(
    selecting: Boolean,
    startSelecting: () -> Unit,
    cancelSelecting: () -> Unit,
    modifier: Modifier,
) {
    MomentoFloatingDock(modifier.windowInsetsPadding(WindowInsets.statusBars).padding(top = 10.dp)) {
        TextButton(onClick = if (selecting) cancelSelecting else startSelecting) {
            Text(if (selecting) "Done" else "Select")
        }
    }
}

fun mergeTimelineGroups(existing: List<TimelineGroup>, next: List<TimelineGroup>): List<TimelineGroup> =
    (existing + next).groupBy { it.date }.map { (date, dateGroups) ->
        TimelineGroup(date, dateGroups.flatMap { it.media }.distinctBy { it.id })
    }

fun flattenTimelineGroups(groups: List<TimelineGroup>): List<TimelineMediaItem> =
    groups.flatMap { group -> group.media.map { media -> TimelineMediaItem(group.date, media) } }

fun timelinePeriodAtIndex(timelineMedia: List<TimelineMediaItem>, index: Int): String? =
    timelineMedia.getOrNull(index)?.period

fun removeTimelineMedia(groups: List<TimelineGroup>, mediaIds: Set<Long>): List<TimelineGroup> =
    groups.mapNotNull { group ->
        val remaining = group.media.filterNot { it.id in mediaIds }
        if (remaining.isEmpty()) null else TimelineGroup(group.date, remaining)
    }
