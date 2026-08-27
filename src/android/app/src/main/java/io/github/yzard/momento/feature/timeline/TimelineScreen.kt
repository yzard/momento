package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.ExperimentalFoundationApi
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
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.rememberDatePickerState
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
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.feature.albums.AlbumAddMediaSheet
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.SelectableMediaThumbnail
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.toggleMediaSelection
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException
import java.time.Instant
import java.time.temporal.ChronoUnit

enum class TimelinePage(
    val mediaType: String?,
    val classification: String?,
) {
    TIMELINE(null, null),
    PHOTOS("image", null),
    VIDEOS("video", null),
    SCREENSHOTS("image", "screenshot"),
    DOCUMENTS("image", "document"),
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

fun datePickerAnchorDate(selectedDateMillis: Long): String =
    Instant.ofEpochMilli(selectedDateMillis).plus(1, ChronoUnit.DAYS).minusMillis(1).toString()

fun shouldAppendTimeline(
    lastVisibleItemIndex: Int,
    totalItemsCount: Int,
    hasOlder: Boolean,
    appending: Boolean,
): Boolean = hasOlder && !appending && totalItemsCount > 0 && lastVisibleItemIndex >= totalItemsCount - 3

fun addToAlbumSelectionLabel(selectedCount: Int): String = "Add to album ($selectedCount)"

fun trashSelectionLabel(selectedCount: Int): String = "Trash ($selectedCount)"

fun compactTimelineSelection(widthDp: Int): Boolean = widthDp < 480

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun TimelineScreen(
    repository: MomentoRepository,
    page: TimelinePage,
    period: TimelinePeriod,
    search: String,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val normalizedSearch = normalizedTimelineSearchQuery(search)
    key(page, period, normalizedSearch) {
        TimelinePageContent(repository, page, period, normalizedSearch, openMedia)
    }
}

@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun TimelinePageContent(
    repository: MomentoRepository,
    page: TimelinePage,
    period: TimelinePeriod,
    search: String,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var groups by remember { mutableStateOf<List<TimelineGroup>?>(null) }
    var olderCursor by remember { mutableStateOf<String?>(null) }
    var newerCursor by remember { mutableStateOf<String?>(null) }
    var hasOlder by remember { mutableStateOf(false) }
    var hasNewer by remember { mutableStateOf(false) }
    var loadingDirection by remember { mutableStateOf<String?>(null) }
    var failedDirection by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var selecting by remember { mutableStateOf(false) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var confirmTrash by remember { mutableStateOf(false) }
    var addingToAlbum by remember { mutableStateOf(false) }
    var selectionError by remember { mutableStateOf<String?>(null) }
    var showDatePicker by remember { mutableStateOf(false) }
    var anchorDate by remember { mutableStateOf(Instant.now().toString()) }
    var refreshVersion by remember { mutableStateOf(0) }
    val scrollKey = remember(page, period, search) { timelineScrollKey(page, period, search) }
    val restoredPosition = remember(scrollKey) { timelineScrollPositions[scrollKey] ?: TimelineScrollPosition(0, 0) }
    val gridState = rememberLazyGridState(restoredPosition.index, restoredPosition.offset)
    val scope = rememberCoroutineScope()

    suspend fun load(reset: Boolean, direction: String) {
        val requestCursor = if (direction == "older") olderCursor else newerCursor
        if (!reset && (loadingDirection != null || requestCursor == null)) return
        if (reset) {
            groups = null
            olderCursor = null
            newerCursor = null
            hasOlder = false
            hasNewer = false
            error = null
            failedDirection = null
            refreshing = true
        } else {
            loadingDirection = direction
        }
        try {
            val timelineResponse = repository.timelinePage(
                cursor = if (reset) null else requestCursor,
                groupBy = period.groupBy,
                search = search,
                mediaType = page.mediaType,
                classification = page.classification,
                direction = direction,
                anchorDate = anchorDate,
            )
            groups = when {
                reset -> timelineResponse.groups
                direction == "older" -> mergeTimelineGroups(groups.orEmpty(), timelineResponse.groups)
                else -> mergeTimelineGroups(timelineResponse.groups, groups.orEmpty())
            }
            if (reset || direction == "older") {
                olderCursor = timelineResponse.nextCursor
                hasOlder = timelineResponse.hasOlder
            }
            if (reset || direction == "newer") {
                newerCursor = timelineResponse.previousCursor
                hasNewer = timelineResponse.hasNewer
            }
            error = null
            failedDirection = null
        } catch (_: IOException) {
            error = "Could not load timeline"
            failedDirection = direction
        } catch (_: HttpException) {
            error = "Could not load timeline"
            failedDirection = direction
        } catch (_: SerializationException) {
            error = "Could not load timeline"
            failedDirection = direction
        } finally {
            loadingDirection = null
            refreshing = false
        }
    }

    suspend fun loadNewer() {
        val firstIndex = gridState.firstVisibleItemIndex
        val firstOffset = gridState.firstVisibleItemScrollOffset
        val existingIds = flattenTimelineGroups(groups.orEmpty()).mapTo(mutableSetOf()) { it.media.id }
        load(reset = false, direction = "newer")
        val addedCount = flattenTimelineGroups(groups.orEmpty()).count { it.media.id !in existingIds }
        if (addedCount > 0) gridState.scrollToItem(firstIndex + addedCount, firstOffset)
    }

    suspend fun moveSelectedToTrash() {
        if (selectedIds.isEmpty()) return
        confirmTrash = false
        try {
            repository.moveToTrash(selectedIds.toList())
            groups = removeTimelineMedia(groups.orEmpty(), selectedIds)
            selectedIds = emptySet()
            selecting = false
        } catch (_: IOException) {
            selectionError = "Could not move the selected media to Trash"
        } catch (_: HttpException) {
            selectionError = "Could not move the selected media to Trash"
        } catch (_: SerializationException) {
            selectionError = "Could not move the selected media to Trash"
        }
    }

    fun refreshTimeline() {
        anchorDate = Instant.now().toString()
        refreshVersion += 1
    }

    LaunchedEffect(repository, page, period, search, anchorDate, refreshVersion) {
        load(reset = true, direction = "older")
    }

    LaunchedEffect(gridState, olderCursor, hasOlder) {
        snapshotFlow {
            val layout = gridState.layoutInfo
            shouldAppendTimeline(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasOlder = hasOlder && error == null,
                appending = loadingDirection != null,
            )
        }.filter { it }.collect { load(reset = false, direction = "older") }
    }

    DisposableEffect(scrollKey, gridState) {
        onDispose {
            timelineScrollPositions[scrollKey] = TimelineScrollPosition(
                index = gridState.firstVisibleItemIndex,
                offset = gridState.firstVisibleItemScrollOffset,
            )
        }
    }

    val currentGroups = groups
    when {
        currentGroups == null && error != null -> ErrorState(requireNotNull(error)) {
            scope.launch { load(reset = true, direction = "older") }
        }
        currentGroups == null -> LoadingState()
        else -> PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = ::refreshTimeline,
            modifier = Modifier.fillMaxSize(),
        ) {
            ContinuousTimelineGrid(
                groups = currentGroups,
                repository = repository,
                gridState = gridState,
                loadingOlder = loadingDirection == "older",
                loadingNewer = loadingDirection == "newer",
                hasNewer = hasNewer,
                error = error,
                failedDirection = failedDirection,
                selecting = selecting,
                selectedIds = selectedIds,
                startSelecting = { selecting = true },
                cancelSelecting = {
                    selecting = false
                    selectedIds = emptySet()
                },
                toggleSelection = { mediaId -> selectedIds = toggleMediaSelection(selectedIds, mediaId) },
                requestTrash = { confirmTrash = true },
                requestAddToAlbum = { addingToAlbum = true },
                loadNewer = { scope.launch { loadNewer() } },
                retryOlder = { scope.launch { load(reset = false, direction = "older") } },
                retryNewer = { scope.launch { loadNewer() } },
                refresh = ::refreshTimeline,
                chooseDate = { showDatePicker = true },
                openMedia = openMedia,
            )
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
    if (showDatePicker) {
        val datePickerState = rememberDatePickerState()
        DatePickerDialog(
            onDismissRequest = { showDatePicker = false },
            confirmButton = {
                TextButton(onClick = {
                    datePickerState.selectedDateMillis?.let { selectedDateMillis ->
                        anchorDate = datePickerAnchorDate(selectedDateMillis)
                        refreshVersion += 1
                    }
                    showDatePicker = false
                }) { Text("Jump") }
            },
            dismissButton = { TextButton(onClick = { showDatePicker = false }) { Text("Cancel") } },
        ) { DatePicker(state = datePickerState) }
    }
}

@Composable
private fun ContinuousTimelineGrid(
    groups: List<TimelineGroup>,
    repository: MomentoRepository,
    gridState: LazyGridState,
    loadingOlder: Boolean,
    loadingNewer: Boolean,
    hasNewer: Boolean,
    error: String?,
    failedDirection: String?,
    selecting: Boolean,
    selectedIds: Set<Long>,
    startSelecting: () -> Unit,
    cancelSelecting: () -> Unit,
    toggleSelection: (Long) -> Unit,
    requestTrash: () -> Unit,
    requestAddToAlbum: () -> Unit,
    loadNewer: () -> Unit,
    retryOlder: () -> Unit,
    retryNewer: () -> Unit,
    refresh: () -> Unit,
    chooseDate: () -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val timelineMedia = remember(groups) { flattenTimelineGroups(groups) }
    if (timelineMedia.isEmpty()) {
        EmptyState("No media yet")
        return
    }
    val allMedia = remember(timelineMedia) { timelineMedia.map { it.media } }
    val leadingItemCount = if (hasNewer || loadingNewer || failedDirection == "newer") 1 else 0
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
            contentPadding = PaddingValues(bottom = 104.dp),
        ) {
            if (leadingItemCount > 0) {
                item(key = "newer", span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TextButton(
                            onClick = if (failedDirection == "newer") retryNewer else loadNewer,
                            enabled = !loadingNewer,
                        ) {
                            Text(
                                when {
                                    loadingNewer -> "Loading newer media"
                                    failedDirection == "newer" -> "Could not load newer media. Retry"
                                    else -> "Load newer media"
                                },
                            )
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
            refresh = refresh,
            chooseDate = chooseDate,
            modifier = Modifier.align(Alignment.TopCenter),
        )
        TimelineSelectionControl(
            selecting = selecting,
            selectedCount = selectedIds.size,
            compact = compactTimelineSelection(maxWidth.value.toInt()),
            startSelecting = startSelecting,
            cancelSelecting = cancelSelecting,
            requestTrash = requestTrash,
            requestAddToAlbum = requestAddToAlbum,
            modifier = Modifier.align(Alignment.TopEnd).padding(end = 12.dp),
        )
    }
}

@Composable
private fun FloatingTimelineHeader(
    label: String,
    refresh: () -> Unit,
    chooseDate: () -> Unit,
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
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = chooseDate) { Icon(Icons.Default.CalendarMonth, "Jump to date") }
            Text(text = label, style = MaterialTheme.typography.labelLarge)
            IconButton(onClick = refresh) { Icon(Icons.Default.Refresh, "Refresh timeline") }
        }
    }
}

@Composable
private fun TimelineSelectionControl(
    selecting: Boolean,
    selectedCount: Int,
    compact: Boolean,
    startSelecting: () -> Unit,
    cancelSelecting: () -> Unit,
    requestTrash: () -> Unit,
    requestAddToAlbum: () -> Unit,
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
        Row {
            if (selecting && selectedCount > 0) {
                if (compact) {
                    Text(
                        selectedCount.toString(),
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.padding(start = 12.dp),
                    )
                    IconButton(onClick = requestAddToAlbum) {
                        Icon(Icons.Default.AddPhotoAlternate, "Add $selectedCount selected media to an album")
                    }
                    IconButton(onClick = requestTrash) {
                        Icon(Icons.Default.Delete, "Move $selectedCount selected media to Trash")
                    }
                } else {
                    TextButton(
                        onClick = requestAddToAlbum,
                        colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                    ) { Text(addToAlbumSelectionLabel(selectedCount)) }
                    TextButton(
                        onClick = requestTrash,
                        colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                    ) {
                        Icon(Icons.Default.Delete, "Move selected media to Trash")
                        Text(trashSelectionLabel(selectedCount))
                    }
                }
            }
            if (selecting) {
                IconButton(onClick = cancelSelecting) { Icon(Icons.Default.Close, "Cancel selection") }
            } else {
                TextButton(
                    onClick = startSelecting,
                    colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                ) { Text("Select") }
            }
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
