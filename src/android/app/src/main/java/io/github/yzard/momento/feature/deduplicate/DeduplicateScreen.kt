package io.github.yzard.momento.feature.deduplicate

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.DeduplicateGroup
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaThumbnail
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException
import java.util.Locale

fun canManageDeduplication(isAdmin: Boolean): Boolean = isAdmin

fun activeDeduplicationState(status: String): Boolean = status in setOf("queued", "running", "cancelling")

fun groupsWithoutKeptMedia(groups: List<DeduplicateGroup>, selectedMediaIds: Set<Long>): List<Long> =
    groups.filter { group ->
        group.items.isNotEmpty() && group.items.all { it.id in selectedMediaIds }
    }.map { it.clusterId }

fun deduplicateTrashConfirmation(selectedCount: Int): String =
    "Move $selectedCount selected ${if (selectedCount == 1) "item" else "items"} to Trash?"

fun deduplicateFileSize(fileSize: Long?): String {
    if (fileSize == null) return "Unknown size"
    if (fileSize < 1024) return "$fileSize B"
    val kibibytes = fileSize / 1024.0
    if (kibibytes < 1024) return "${kibibytes.toLong()} KiB"
    return String.format(Locale.ENGLISH, "%.1f MiB", kibibytes / 1024.0)
}

fun deduplicateDimensions(media: Media): String =
    if (media.width == null || media.height == null) "Unknown dimensions" else "${media.width} × ${media.height}"

fun deduplicateColumns(widthDp: Int): Int = when {
    widthDp < 560 -> 2
    widthDp < 840 -> 3
    else -> 4
}

fun compactDeduplicateActions(widthDp: Int): Boolean = widthDp < 460

@Composable
fun DeduplicateScreen(
    repository: MomentoRepository,
    isAdmin: Boolean,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var status by remember(repository) { mutableStateOf("idle") }
    var groups by remember(repository) { mutableStateOf<List<DeduplicateGroup>?>(null) }
    var nextCursor by remember(repository) { mutableStateOf<String?>(null) }
    var hasMore by remember(repository) { mutableStateOf(false) }
    var totalGroups by remember(repository) { mutableStateOf(0L) }
    var totalMedia by remember(repository) { mutableStateOf(0L) }
    var selectedMediaIds by remember(repository) { mutableStateOf<Set<Long>>(emptySet()) }
    var loadingMore by remember(repository) { mutableStateOf(false) }
    var working by remember(repository) { mutableStateOf(false) }
    var confirmTrash by remember { mutableStateOf(false) }
    var confirmAdminAction by remember { mutableStateOf<String?>(null) }
    var error by remember(repository) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    suspend fun loadGroups(reset: Boolean) {
        if (loadingMore || (!reset && (!hasMore || nextCursor == null))) return
        if (reset) {
            groups = null
            nextCursor = null
            hasMore = false
        } else {
            loadingMore = true
        }
        try {
            val response = repository.duplicateGroups(if (reset) null else nextCursor)
            groups = if (reset) {
                response.groups
            } else {
                appendDeduplicateGroups(groups.orEmpty(), response.groups)
            }
            nextCursor = response.nextCursor
            hasMore = response.hasMore
            totalGroups = response.totalGroups
            totalMedia = response.totalMedia
            error = null
        } catch (_: IOException) {
            error = "Could not load duplicate groups"
        } catch (_: HttpException) {
            error = "Could not load duplicate groups"
        } catch (_: SerializationException) {
            error = "Could not load duplicate groups"
        } finally {
            loadingMore = false
        }
    }

    suspend fun loadStatus() {
        if (!isAdmin) return
        try {
            status = repository.aiStatus().deduplicate.status
        } catch (_: IOException) {
            error = "Could not load deduplication status"
        } catch (_: HttpException) {
            error = "Could not load deduplication status"
        } catch (_: SerializationException) {
            error = "Could not load deduplication status"
        }
    }

    suspend fun runAdminAction(action: String) {
        if (working) return
        working = true
        try {
            when (action) {
                "start" -> repository.startAiFeature("deduplicate")
                "cancel" -> repository.cancelAiFeature("deduplicate")
                "clean" -> repository.cleanAiFeature("deduplicate")
                else -> kotlin.error("Unknown deduplication action $action")
            }
            error = null
            loadStatus()
            loadGroups(true)
        } catch (_: IOException) {
            error = "Could not $action deduplication"
        } catch (_: HttpException) {
            error = "Could not $action deduplication"
        } catch (_: SerializationException) {
            error = "Could not $action deduplication"
        } finally {
            working = false
        }
    }

    suspend fun moveSelectedToTrash() {
        if (working || selectedMediaIds.isEmpty()) return
        val unsafeGroups = groupsWithoutKeptMedia(groups.orEmpty(), selectedMediaIds)
        if (unsafeGroups.isNotEmpty()) {
            error = "Keep at least one item in every similar group before moving media to Trash"
            confirmTrash = false
            return
        }
        working = true
        try {
            repository.moveToTrash(selectedMediaIds.toList())
            selectedMediaIds = emptySet()
            confirmTrash = false
            loadGroups(true)
        } catch (_: IOException) {
            error = "Could not move selected media to Trash"
        } catch (_: HttpException) {
            error = "Could not move selected media to Trash"
        } catch (_: SerializationException) {
            error = "Could not move selected media to Trash"
        } finally {
            working = false
        }
    }

    LaunchedEffect(repository) { loadGroups(true) }
    LaunchedEffect(repository, isAdmin) {
        while (isAdmin) {
            loadStatus()
            delay(2_000)
        }
    }

    val currentGroups = groups
    when {
        currentGroups == null && error != null -> ErrorState(requireNotNull(error)) { scope.launch { loadGroups(true) } }
        currentGroups == null -> LoadingState()
        else -> Box(Modifier.fillMaxSize()) {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(top = 64.dp, bottom = if (selectedMediaIds.isEmpty()) 96.dp else 180.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                item {
                    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("$totalGroups similar groups · $totalMedia media", style = MaterialTheme.typography.titleMedium)
                        if (isAdmin) {
                            Text("Processing: $status", color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(
                                    onClick = {
                                        if (activeDeduplicationState(status)) confirmAdminAction = "cancel"
                                        else scope.launch { runAdminAction("start") }
                                    },
                                    enabled = !working,
                                ) { Text(if (activeDeduplicationState(status)) "Cancel" else "Start") }
                                OutlinedButton(
                                    onClick = { confirmAdminAction = "clean" },
                                    enabled = !working && !activeDeduplicationState(status),
                                ) { Text("Clean") }
                            }
                        }
                        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    }
                }
                if (currentGroups.isEmpty()) {
                    item { EmptyState("No similar media groups") }
                }
                items(currentGroups, key = { it.clusterId }) { group ->
                    DuplicateGroupCard(
                        group = group,
                        repository = repository,
                        selectedMediaIds = selectedMediaIds,
                        openMedia = { media ->
                            openMedia(group.items, group.items.indexOfFirst { it.id == media.id }.coerceAtLeast(0))
                        },
                        toggleSelection = { mediaId ->
                            selectedMediaIds = if (mediaId in selectedMediaIds) {
                                selectedMediaIds - mediaId
                            } else {
                                selectedMediaIds + mediaId
                            }
                        },
                    )
                }
                if (hasMore) {
                    item {
                        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                            TextButton(
                                onClick = { scope.launch { loadGroups(false) } },
                                enabled = !loadingMore,
                            ) { Text(if (loadingMore) "Loading more" else "Load more groups") }
                        }
                    }
                }
            }
            if (selectedMediaIds.isNotEmpty()) {
                Surface(
                    modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(12.dp),
                    shape = RoundedCornerShape(18.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    shadowElevation = 8.dp,
                ) {
                    BoxWithConstraints(Modifier.fillMaxWidth().padding(12.dp)) {
                        if (compactDeduplicateActions(maxWidth.value.toInt())) {
                            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text("${selectedMediaIds.size} selected", fontWeight = FontWeight.Bold)
                                Text("Unselected items are kept", style = MaterialTheme.typography.bodySmall)
                                Row(Modifier.align(Alignment.End)) {
                                    TextButton(onClick = { selectedMediaIds = emptySet() }, enabled = !working) {
                                        Text("Clear")
                                    }
                                    Button(onClick = { confirmTrash = true }, enabled = !working) {
                                        Text("Move to Trash")
                                    }
                                }
                            }
                        } else {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Column(Modifier.weight(1f)) {
                                    Text("${selectedMediaIds.size} selected", fontWeight = FontWeight.Bold)
                                    Text("Unselected items are kept", style = MaterialTheme.typography.bodySmall)
                                }
                                TextButton(onClick = { selectedMediaIds = emptySet() }, enabled = !working) { Text("Clear") }
                                Button(onClick = { confirmTrash = true }, enabled = !working) { Text("Move to Trash") }
                            }
                        }
                    }
                }
            }
        }
    }

    if (confirmTrash) {
        AlertDialog(
            onDismissRequest = { if (!working) confirmTrash = false },
            title = { Text(deduplicateTrashConfirmation(selectedMediaIds.size)) },
            text = { Text("Only the selected media will move. At least one item must remain in every group.") },
            confirmButton = {
                TextButton(onClick = { scope.launch { moveSelectedToTrash() } }, enabled = !working) {
                    Text(if (working) "Moving" else "Move ${selectedMediaIds.size}")
                }
            },
            dismissButton = { TextButton(onClick = { confirmTrash = false }, enabled = !working) { Text("Cancel") } },
        )
    }
    confirmAdminAction?.let { action ->
        AlertDialog(
            onDismissRequest = { confirmAdminAction = null },
            title = { Text(if (action == "clean") "Clean deduplication data?" else "Cancel deduplication?") },
            text = { Text(if (action == "clean") "Stored similarity results and duplicate groups will be removed." else "Queued and active deduplication jobs will be cancelled.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmAdminAction = null
                    scope.launch { runAdminAction(action) }
                }) { Text(if (action == "clean") "Clean" else "Cancel jobs") }
            },
            dismissButton = { TextButton(onClick = { confirmAdminAction = null }) { Text("Keep current data") } },
        )
    }
}

@Composable
private fun DuplicateGroupCard(
    group: DeduplicateGroup,
    repository: MomentoRepository,
    selectedMediaIds: Set<Long>,
    openMedia: (Media) -> Unit,
    toggleSelection: (Long) -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.fillMaxWidth().padding(12.dp)) {
            Text("Similar group ${group.clusterId}", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text("${group.items.size} items to compare", color = MaterialTheme.colorScheme.onSurfaceVariant)
            BoxWithConstraints(Modifier.fillMaxWidth().padding(top = 10.dp)) {
                val columns = deduplicateColumns(maxWidth.value.toInt())
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    group.items.chunked(columns).forEach { mediaRow ->
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            mediaRow.forEach { media ->
                                DuplicateMediaCard(
                                    media = media,
                                    repository = repository,
                                    selected = media.id in selectedMediaIds,
                                    open = { openMedia(media) },
                                    toggleSelection = { toggleSelection(media.id) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            repeat(columns - mediaRow.size) { Box(Modifier.weight(1f)) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DuplicateMediaCard(
    media: Media,
    repository: MomentoRepository,
    selected: Boolean,
    open: () -> Unit,
    toggleSelection: () -> Unit,
    modifier: Modifier,
) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(
            containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
        ),
    ) {
        Box {
            MediaThumbnail(
                media = media,
                repository = repository,
                trashed = false,
                modifier = Modifier.fillMaxWidth().aspectRatio(1f).clickable(onClick = open),
            )
            IconButton(
                onClick = toggleSelection,
                modifier = Modifier.align(Alignment.TopEnd),
            ) {
                Icon(
                    imageVector = if (selected) Icons.Default.CheckCircle else Icons.Default.RadioButtonUnchecked,
                    contentDescription = if (selected) "Deselect ${media.originalFilename}" else "Select ${media.originalFilename}",
                    tint = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                )
            }
        }
        Column(Modifier.padding(10.dp)) {
            Text(media.originalFilename, maxLines = 1, style = MaterialTheme.typography.labelLarge)
            Text(deduplicateFileSize(media.fileSize), style = MaterialTheme.typography.bodySmall)
            Text(deduplicateDimensions(media), style = MaterialTheme.typography.bodySmall)
            Text(media.dateTaken ?: media.createdAt, maxLines = 1, style = MaterialTheme.typography.bodySmall)
        }
    }
}

fun appendDeduplicateGroups(
    existing: List<DeduplicateGroup>,
    page: List<DeduplicateGroup>,
): List<DeduplicateGroup> = existing + page.filter { candidate ->
    existing.none { it.clusterId == candidate.clusterId }
}
