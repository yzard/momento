package io.github.yzard.momento.feature.faces

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import coil.compose.AsyncImage
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.FaceGroup
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

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
    var detail by remember(repository) { mutableStateOf<FaceGroup?>(null) }
    var loading by remember(repository) { mutableStateOf(false) }
    var working by remember(repository) { mutableStateOf(false) }
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
    BackHandler(enabled = detail != null) { detail = null }

    val selectedGroup = detail
    if (selectedGroup != null) {
        FaceGroupDetailScreen(repository, selectedGroup, openMedia)
        return
    }

    when {
        groups == null && error != null -> ErrorState(error!!) { scope.launch { loadGroups(true) } }
        groups == null -> LoadingState()
        groups!!.isEmpty() -> EmptyState("No faces yet")
        else -> Column {
            if (isAdmin && selectedIds.size >= 2) {
                Button(
                    onClick = { scope.launch { mergeSelected() } },
                    enabled = !working,
                ) {
                    Text(if (working) "Merging..." else "Merge selected")
                }
            }
            error?.let { Text(it) }
            LazyColumn {
                items(groups!!, key = { it.faceGroupId }) { group ->
                    FaceRow(
                        group = group,
                        repository = repository,
                        selected = group.faceGroupId in selectedIds,
                        selectable = isAdmin,
                        open = { detail = group },
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
                    item {
                        TextButton(
                            onClick = { scope.launch { loadGroups(false) } },
                            enabled = !loading,
                        ) {
                            Text(if (loading) "Loading more..." else "Load more")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun FaceGroupDetailScreen(
    repository: MomentoRepository,
    group: FaceGroup,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var media by remember(repository, group.faceGroupId) { mutableStateOf<List<Media>?>(null) }
    var error by remember(repository, group.faceGroupId) { mutableStateOf<String?>(null) }

    LaunchedEffect(repository, group.faceGroupId) {
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
        media == null && error != null -> ErrorState(error!!) {}
        media == null -> LoadingState()
        else -> Column {
            Text("Person ${group.faceGroupId}")
            MediaGrid(media!!, repository) { item -> openMedia(media!!, media!!.indexOf(item)) }
        }
    }
}

@Composable
private fun FaceRow(
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
    ListItem(
        headlineContent = { Text("Person ${group.faceGroupId}") },
        supportingContent = { Text("${group.mediaCount} photos") },
        leadingContent = { AsyncImage(image, null) },
        trailingContent = if (selectable) {
            {
                TextButton(onClick = toggleSelection) {
                    Text(if (selected) "Selected" else "Select")
                }
            }
        } else {
            null
        },
        modifier = Modifier.clickable(onClick = open),
    )
}

fun appendFaceGroups(existing: List<FaceGroup>, page: List<FaceGroup>): List<FaceGroup> =
    existing + page.filter { candidate -> existing.none { it.faceGroupId == candidate.faceGroupId } }
