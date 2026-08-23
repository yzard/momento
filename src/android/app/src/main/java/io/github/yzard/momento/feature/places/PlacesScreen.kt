package io.github.yzard.momento.feature.places

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.Place
import io.github.yzard.momento.feature.media.MediaGrid

@Composable fun PlacesScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) { var places by remember { mutableStateOf<List<Place>?>(null) }; var selected by remember { mutableStateOf<Place?>(null) }; LaunchedEffect(Unit) { runCatching { repository.places() }.onSuccess { places = it } }; if (selected != null) PlaceDetailScreen(repository, selected!!.placeId, openMedia) else LazyColumn(Modifier.fillMaxSize()) { items(places ?: emptyList()) { place -> ListItem({ Text(place.city) }, supportingContent = { Text(listOfNotNull(place.state, place.country).joinToString(", ")) }, trailingContent = { Text(place.mediaCount.toString()) }, modifier = Modifier.clickable { selected = place }) } } }
@Composable private fun PlaceDetailScreen(repository: MomentoRepository, placeId: String, openMedia: (List<Media>, Int) -> Unit) { var media by remember { mutableStateOf<List<Media>>(emptyList()) }; var nextCursor by remember { mutableStateOf<String?>(null) }; var more by remember { mutableStateOf(true) }; var requestCursor by remember { mutableStateOf<String?>(null) }; LaunchedEffect(placeId, requestCursor) { runCatching { repository.place(placeId, requestCursor) }.onSuccess { response -> media = appendPlaceMedia(media, response.media); nextCursor = response.nextCursor; more = response.hasMore } }; Column { MediaGrid(media, repository) { item -> openMedia(media, media.indexOf(item)) }; if (more && nextCursor != null) Text("Load more", Modifier.clickable { requestCursor = nextCursor }) } }
fun appendPlaceMedia(existing: List<Media>, page: List<Media>): List<Media> = existing + page.filter { candidate -> existing.none { it.id == candidate.id } }
