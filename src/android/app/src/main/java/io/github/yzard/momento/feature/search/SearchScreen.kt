package io.github.yzard.momento.feature.search

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.delay

fun normalizedSearchQuery(value: String): String = value.trim()
@Composable fun SearchScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) { var query by remember { mutableStateOf("") }; var results by remember { mutableStateOf<List<Media>?>(null) }; var error by remember { mutableStateOf<String?>(null) }; val normalized = normalizedSearchQuery(query); LaunchedEffect(normalized) { if (normalized.isBlank()) { results = emptyList(); error = null; return@LaunchedEffect }; delay(350); results = null; runCatching { repository.search(normalized) }.onSuccess { results = it; error = null }.onFailure { error = "Search failed" } }; Column { OutlinedTextField(query, { query = it }, label = { Text("Search your photos") }, modifier = Modifier.fillMaxWidth().padding(16.dp)); when { normalized.isBlank() -> EmptyState("Search by words found in your photos"); error != null -> ErrorState(error!!) { query = "$query " }; results == null -> LoadingState(); results!!.isEmpty() -> EmptyState("No matching photos"); else -> MediaGrid(results!!, repository) { item -> openMedia(results!!, results!!.indexOf(item)) } } } }
