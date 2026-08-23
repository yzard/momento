package io.github.yzard.momento.feature.timeline

import android.content.res.Configuration
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaRow
import io.github.yzard.momento.feature.media.adaptiveGridColumns
import io.github.yzard.momento.feature.media.mediaCellWidth
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import java.io.IOException
import retrofit2.HttpException

data class TimelineSelection(val groupBy: String, val filter: String)

fun timelineFilters(filter: String): Pair<String?, String?> = when (filter) {
    "Photos" -> "image" to null
    "Videos" -> "video" to null
    "Screenshots" -> null to "screenshot"
    "Documents" -> null to "document"
    else -> null to null
}

fun shouldAppendTimeline(
    lastVisibleItemIndex: Int,
    totalItemsCount: Int,
    hasOlder: Boolean,
    appending: Boolean,
): Boolean = hasOlder && !appending && totalItemsCount > 0 && lastVisibleItemIndex >= totalItemsCount - 3

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun TimelineScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) {
    var selection by remember { mutableStateOf(TimelineSelection("day", "All")) }
    var groups by remember { mutableStateOf<List<TimelineGroup>?>(null) }
    var cursor by remember { mutableStateOf<String?>(null) }
    var hasOlder by remember { mutableStateOf(false) }
    var appending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val portrait = LocalConfiguration.current.orientation == Configuration.ORIENTATION_PORTRAIT

    suspend fun load(reset: Boolean) {
        if (!reset && (appending || !hasOlder || cursor == null)) return
        if (!reset) appending = true
        val requestedSelection = selection
        val filters = timelineFilters(selection.filter)
        try {
            val page = repository.timelinePage(
                cursor = if (reset) null else cursor,
                groupBy = selection.groupBy,
                mediaType = filters.first,
                classification = filters.second,
            )
            if (selection != requestedSelection) return
            groups = if (reset) page.groups else mergeTimelineGroups(groups.orEmpty(), page.groups)
            cursor = page.nextCursor
            hasOlder = page.hasOlder
            error = null
        } catch (_: IOException) {
            error = "Could not load photos"
        } catch (_: HttpException) {
            error = "Could not load photos"
        } finally {
            appending = false
        }
    }

    LaunchedEffect(selection) {
        listState.scrollToItem(0)
        groups = null
        cursor = null
        hasOlder = false
        error = null
        load(true)
    }

    LaunchedEffect(listState, cursor, hasOlder) {
        snapshotFlow {
            val layout = listState.layoutInfo
            shouldAppendTimeline(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasOlder = hasOlder && error == null,
                appending = appending,
            )
        }.filter { it }.collect { load(false) }
    }

    Column(Modifier.fillMaxSize()) {
        TimelineTabs(selection) { selection = it }
        val current = groups
        when {
            current == null && error != null -> ErrorState(error!!) { scope.launch { load(true) } }
            current == null -> LoadingState()
            else -> BoxWithConstraints(Modifier.weight(1f)) {
                val columns = adaptiveGridColumns(maxWidth.value.toInt())
                val cellWidth = mediaCellWidth(maxWidth.value, columns, 1f).dp
                val allMedia = remember(current) { current.flatMap { it.media } }
                LazyColumn(state = listState) {
                    current.forEach { group ->
                        if (portrait) {
                            stickyHeader(key = "header-${group.date}") {
                                FloatingTimelineHeader(group.date)
                            }
                        } else {
                            item(key = "header-${group.date}") {
                                Text(group.date, Modifier.padding(16.dp, 16.dp, 16.dp, 8.dp))
                            }
                        }
                        group.media.chunked(columns).forEachIndexed { rowIndex, row ->
                            item(key = "${group.date}-$rowIndex-${row.first().id}") {
                                MediaRow(row, repository, columns, cellWidth) { media ->
                                    openMedia(allMedia, allMedia.indexOfFirst { it.id == media.id })
                                }
                            }
                        }
                    }
                    if (appending) {
                        item(key = "append-loading") {
                            Box(
                                modifier = Modifier.fillMaxWidth().padding(20.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                CircularProgressIndicator()
                            }
                        }
                    } else if (error != null) {
                        item(key = "append-error") {
                            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                                TextButton(onClick = { scope.launch { error = null; load(false) } }) { Text("Could not load more photos. Retry") }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun TimelineTabs(selection: TimelineSelection, select: (TimelineSelection) -> Unit) {
    val groupings = listOf("day", "week", "month", "year")
    val filters = listOf("All", "Photos", "Videos", "Screenshots", "Documents")
    ScrollableTabRow(groupings.indexOf(selection.groupBy)) {
        groupings.forEach { value ->
            Tab(
                selected = selection.groupBy == value,
                onClick = { select(selection.copy(groupBy = value)) },
                text = { Text(value.replaceFirstChar { it.uppercase() }) },
            )
        }
    }
    ScrollableTabRow(filters.indexOf(selection.filter)) {
        filters.forEach { value ->
            Tab(
                selected = selection.filter == value,
                onClick = { select(selection.copy(filter = value)) },
                text = { Text(value) },
            )
        }
    }
}

@Composable
private fun FloatingTimelineHeader(label: String) {
    Box(
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
        contentAlignment = Alignment.TopCenter,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.surface.copy(alpha = 0.94f),
            shape = MaterialTheme.shapes.extraLarge,
            shadowElevation = 6.dp,
            tonalElevation = 3.dp,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
        }
    }
}

fun mergeTimelineGroups(existing: List<TimelineGroup>, next: List<TimelineGroup>): List<TimelineGroup> =
    (existing + next).groupBy { it.date }.map { (date, groups) ->
        TimelineGroup(date, groups.flatMap { it.media }.distinctBy { it.id })
    }
