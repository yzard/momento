package io.github.yzard.momento.feature.media

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MediaGrid(media: List<Media>, repository: MomentoRepository, select: (Media) -> Unit) {
    BoxWithConstraints {
        val columns = adaptiveGridColumns(maxWidth.value.toInt())
        val cellWidth = mediaCellWidth(maxWidth.value, columns, 1f).dp
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            media.chunked(columns).forEach { row ->
                MediaRow(row, repository, columns, cellWidth, select)
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MediaRow(
    media: List<Media>,
    repository: MomentoRepository,
    columns: Int,
    cellWidth: androidx.compose.ui.unit.Dp,
    select: (Media) -> Unit,
) {
    FlowRow(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(1.dp),
        maxItemsInEachRow = columns,
    ) {
        media.forEach { item ->
            MediaThumbnail(
                media = item,
                repository = repository,
                trashed = false,
                modifier = Modifier
                    .width(cellWidth)
                    .aspectRatio(1f)
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .clickable { select(item) },
            )
        }
    }
}

fun mediaRows(ids: List<Long>, columns: Int): List<List<Long>> = ids.chunked(columns)

fun mediaCellWidth(containerWidth: Float, columns: Int, gap: Float): Float {
    require(columns > 0) { "columns must be positive" }
    return ((containerWidth - gap * (columns - 1)) / columns).coerceAtLeast(0f)
}

@Composable
fun MediaThumbnail(media: Media, repository: MomentoRepository, trashed: Boolean, modifier: Modifier) {
    val context = LocalContext.current
    val url by produceState<String?>(null, media.id, trashed) {
        value = if (trashed) repository.trashThumbnailUrl(media.id) else repository.thumbnailUrl(media.id, true)
    }
    AsyncImage(
        model = url?.let { ImageRequest.Builder(context).data(it).build() },
        imageLoader = repository.authenticatedImageLoader(context),
        contentDescription = media.originalFilename,
        contentScale = ContentScale.Crop,
        modifier = modifier,
    )
}

@Composable
fun SelectableMediaThumbnail(
    media: Media,
    repository: MomentoRepository,
    trashed: Boolean,
    selected: Boolean,
    modifier: Modifier,
) {
    Box(modifier) {
        MediaThumbnail(media, repository, trashed, Modifier.fillMaxSize())
        if (selected) {
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.38f)))
            Icon(
                imageVector = Icons.Default.CheckCircle,
                contentDescription = "Selected",
                tint = Color.White,
                modifier = Modifier.align(androidx.compose.ui.Alignment.TopEnd).padding(8.dp),
            )
        }
    }
}

fun toggleMediaSelection(selectedIds: Set<Long>, mediaId: Long): Set<Long> =
    if (mediaId in selectedIds) selectedIds - mediaId else selectedIds + mediaId
