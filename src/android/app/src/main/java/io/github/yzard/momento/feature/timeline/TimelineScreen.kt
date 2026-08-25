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
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.AlertDialog
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.SelectableMediaThumbnail
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.toggleMediaSelection
import io.github.yzard.momento.feature.albums.AlbumAddMediaSheet
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import java.io.IOException
import java.time.Instant
import retrofit2.HttpException

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

fun normalizedTimelineSearchQuery(value: String): String = value.trim()

fun shouldAppendTimeline(
    lastVisibleItemIndex: Int,
    totalItemsCount: Int,
    hasOlder: Boolean,
    appending: Boolean,
): Boolean = hasOlder && !appending && totalItemsCount > 0 && lastVisibleItemIndex >= totalItemsCount - 3

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
    var cursor by remember { mutableStateOf<String?>(null) }
    var hasOlder by remember { mutableStateOf(false) }
    var appending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var selecting by remember { mutableStateOf(false) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var confirmTrash by remember { mutableStateOf(false) }
    var addingToAlbum by remember { mutableStateOf(false) }
    var selectionError by remember { mutableStateOf<String?>(null) }
    val gridState = rememberLazyGridState()
    val scope = rememberCoroutineScope()
    val anchorDate = remember(page, period, search) { Instant.now().toString() }

    suspend fun load(reset: Boolean) {
        if (!reset && (appending || !hasOlder || cursor == null)) return
        if (reset) {
            groups = null
            cursor = null
            hasOlder = false
            error = null
        } else {
            appending = true
        }
        try {
            val timelineResponse = repository.timelinePage(
                cursor = if (reset) null else cursor,
                groupBy = period.groupBy,
                search = search,
                mediaType = page.mediaType,
                classification = page.classification,
                anchorDate = anchorDate,
            )
            groups = if (reset) {
                timelineResponse.groups
            } else {
                mergeTimelineGroups(groups.orEmpty(), timelineResponse.groups)
            }
            cursor = timelineResponse.nextCursor
            hasOlder = timelineResponse.hasOlder
            error = null
        } catch (_: IOException) {
            error = "Could not load timeline"
        } catch (_: HttpException) {
            error = "Could not load timeline"
        } catch (_: SerializationException) {
            error = "Could not load timeline"
        } finally {
            appending = false
        }
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

    LaunchedEffect(repository, page, period, search, anchorDate) {
        load(true)
    }

    LaunchedEffect(gridState, cursor, hasOlder) {
        snapshotFlow {
            val layout = gridState.layoutInfo
            shouldAppendTimeline(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasOlder = hasOlder && error == null,
                appending = appending,
            )
        }.filter { it }.collect { load(false) }
    }

    val current = groups
    when {
        current == null && error != null -> ErrorState(error!!) { scope.launch { load(true) } }
        current == null -> LoadingState()
        else -> ContinuousTimelineGrid(
            groups = current,
            repository = repository,
            gridState = gridState,
            appending = appending,
            error = error,
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
            retry = { scope.launch { error = null; load(false) } },
            openMedia = openMedia,
        )
    }


    if (confirmTrash) {
        AlertDialog(
            onDismissRequest = { confirmTrash = false },
            title = { Text("Move to Trash?") },
            text = { Text("Move ${selectedIds.size} selected item${if (selectedIds.size == 1) "" else "s"} to Trash?") },
            confirmButton = {
                TextButton(onClick = { scope.launch { moveSelectedToTrash() } }) { Text("Move") }
            },
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
}

@Composable
private fun ContinuousTimelineGrid(
    groups: List<TimelineGroup>,
    repository: MomentoRepository,
    gridState: androidx.compose.foundation.lazy.grid.LazyGridState,
    appending: Boolean,
    error: String?,
    selecting: Boolean,
    selectedIds: Set<Long>,
    startSelecting: () -> Unit,
    cancelSelecting: () -> Unit,
    toggleSelection: (Long) -> Unit,
    requestTrash: () -> Unit,
    requestAddToAlbum: () -> Unit,
    retry: () -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val timelineMedia = remember(groups) { flattenTimelineGroups(groups) }
    if (timelineMedia.isEmpty()) {
        EmptyState("No media yet")
        return
    }
    val allMedia = remember(timelineMedia) { timelineMedia.map { it.media } }
    val visiblePeriod by remember(timelineMedia, gridState) {
        derivedStateOf {
            timelinePeriodAtIndex(timelineMedia, gridState.firstVisibleItemIndex)
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
                                openMedia(
                                    allMedia,
                                    allMedia.indexOfFirst { it.id == timelineItem.media.id },
                                )
                            }
                        },
                )
            }
            if (appending) {
                item(key = "append-loading", span = { GridItemSpan(maxLineSpan) }) {
                    Box(
                        modifier = Modifier.fillMaxWidth().padding(20.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        CircularProgressIndicator()
                    }
                }
            } else if (error != null) {
                item(key = "append-error", span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TextButton(onClick = retry) { Text("Could not load more media. Retry") }
                    }
                }
            }
        }
        FloatingTimelineHeader(
            label = visiblePeriod,
            modifier = Modifier.align(Alignment.TopCenter),
        )
        TimelineSelectionControl(
            selecting = selecting,
            selectedCount = selectedIds.size,
            startSelecting = startSelecting,
            cancelSelecting = cancelSelecting,
            requestTrash = requestTrash,
            requestAddToAlbum = requestAddToAlbum,
            modifier = Modifier.align(Alignment.TopEnd).padding(end = 12.dp),
        )
    }
}

@Composable
private fun FloatingTimelineHeader(label: String, modifier: Modifier) {
    val floatingColors = momentoFloatingControlColors()
    Surface(
        modifier = modifier
            .windowInsetsPadding(WindowInsets.statusBars)
            .padding(top = 10.dp),
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
        shadowElevation = 3.dp,
        tonalElevation = 1.dp,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
    }
}

@Composable
private fun TimelineSelectionControl(
    selecting: Boolean,
    selectedCount: Int,
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
                TextButton(
                    onClick = requestAddToAlbum,
                    colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                ) { Text("Album $selectedCount") }
                TextButton(
                    onClick = requestTrash,
                    colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                ) {
                    Icon(Icons.Default.Delete, "Move selected media to Trash")
                    Text("$selectedCount")
                }
            }
            if (selecting) {
                IconButton(onClick = cancelSelecting) {
                    Icon(Icons.Default.Close, "Cancel selection")
                }
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
    (existing + next).groupBy { it.date }.map { (date, groups) ->
        TimelineGroup(date, groups.flatMap { it.media }.distinctBy { it.id })
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
