package io.github.yzard.momento.feature.viewer

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import coil.compose.AsyncImage
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import kotlinx.coroutines.launch

fun viewerIndex(index: Int, change: Int, size: Int): Int = (index + change).coerceIn(0, size - 1)
fun removeViewedMedia(media: List<Media>, index: Int): Pair<List<Media>, Int> { val remaining = media.filterIndexed { itemIndex, _ -> itemIndex != index }; return remaining to index.coerceAtMost((remaining.lastIndex).coerceAtLeast(0)) }
fun mediaMetadata(media: Media): String = listOfNotNull(media.dateTaken, media.mediaType, media.width?.let { "${it}x${media.height ?: "?"}" }, media.locationCity, media.fileSize?.let { "$it bytes" }).joinToString(" • ")
@Composable fun ViewerScreen(media: List<Media>, initialIndex: Int, repository: MomentoRepository, close: () -> Unit) {
    var items by remember { mutableStateOf(media) }
    var index by remember { mutableStateOf(initialIndex.coerceIn(0, media.lastIndex)) }
    var scale by remember { mutableFloatStateOf(1f) }
    var confirm by remember { mutableStateOf(false) }
    val item = items[index]
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val url by produceState<String?>(null, item.id) { value = repository.previewUrl(item.id) }
    val transform = rememberTransformableState { zoom, _, _ -> scale = (scale * zoom).coerceIn(1f, 5f) }
    BackHandler(onBack = close)
    Column(Modifier.fillMaxSize()) {
        TextButton(close) { Text("Back") }
        Text(item.originalFilename)
        AsyncImage(url, item.originalFilename, imageLoader = repository.authenticatedImageLoader(context), modifier = Modifier.weight(1f).transformable(transform).graphicsLayer(scaleX = scale, scaleY = scale))
        Text(mediaMetadata(item))
        TextButton({ index = viewerIndex(index, -1, items.size) }, enabled = index > 0) { Text("Previous") }
        TextButton({ index = viewerIndex(index, 1, items.size) }, enabled = index < items.lastIndex) { Text("Next") }
        Button({ confirm = true }) { Text("Move to trash") }
    }
    if (confirm) {
        AlertDialog(onDismissRequest = { confirm = false }, title = { Text("Move to trash?") }, confirmButton = {
            TextButton({ scope.launch { repository.moveToTrash(listOf(item.id)); val result = removeViewedMedia(items, index); items = result.first; index = result.second; if (items.isEmpty()) close(); confirm = false } }) { Text("Move") }
        }, dismissButton = { TextButton({ confirm = false }) { Text("Cancel") } })
    }
}
