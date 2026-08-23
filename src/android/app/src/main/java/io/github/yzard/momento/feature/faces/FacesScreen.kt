package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
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
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.launch

@Composable fun FacesScreen(repository: MomentoRepository, isAdmin: Boolean, openMedia: (List<Media>, Int) -> Unit) { var groups by remember { mutableStateOf<List<FaceGroup>>(emptyList()) }; var selected by remember { mutableStateOf<Set<Long>>(emptySet()) }; var detail by remember { mutableStateOf<FaceGroup?>(null) }; val scope = rememberCoroutineScope(); fun reload() { scope.launch { groups = repository.faces(); selected = emptySet() } }; LaunchedEffect(Unit) { reload() }; if (detail != null) { var media by remember { mutableStateOf<List<Media>>(emptyList()) }; LaunchedEffect(detail!!.faceGroupId) { media = repository.faceGroup(detail!!.faceGroupId).media }; Column { Text("Person ${detail!!.faceGroupId}"); MediaGrid(media, repository) { item -> openMedia(media, media.indexOf(item)) } }; return }; Column { if (isAdmin && selected.size >= 2) Button({ scope.launch { repository.mergeFaces(selected.toList()); reload() } }) { Text("Merge selected") }; LazyColumn { items(groups, key = { it.faceGroupId }) { group -> FaceRow(group, repository, Modifier.clickable { selected = if (group.faceGroupId in selected) selected - group.faceGroupId else selected + group.faceGroupId; detail = group }) } } } }
@Composable private fun FaceRow(group: FaceGroup, repository: MomentoRepository, modifier: androidx.compose.ui.Modifier) { var image by remember { mutableStateOf<ByteArray?>(null) }; LaunchedEffect(group.faceGroupId) { image = runCatching { repository.faceThumbnail(group.faceGroupId) }.getOrNull() }; ListItem({ Text("Person ${group.faceGroupId}") }, supportingContent = { Text("${group.mediaCount} photos") }, leadingContent = { AsyncImage(image, null) }, modifier = modifier) }
