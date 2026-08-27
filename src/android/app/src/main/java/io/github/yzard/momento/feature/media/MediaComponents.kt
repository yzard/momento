package io.github.yzard.momento.feature.media

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
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

@Composable
fun MediaGrid(
    media: List<Media>,
    repository: MomentoRepository,
    selectedMediaIds: Set<Long>,
    contentPadding: PaddingValues,
    headerContent: (@Composable () -> Unit)?,
    footerContent: (@Composable () -> Unit)?,
    modifier: Modifier,
    select: (Media) -> Unit,
) {
    LazyMediaGrid(
        entries = media,
        entryKey = { mediaItem -> mediaItem.id },
        entrySelected = { mediaItem -> mediaItem.id in selectedMediaIds },
        contentPadding = contentPadding,
        headerContent = headerContent,
        footerContent = footerContent,
        modifier = modifier,
    ) { mediaItem, selected ->
        SelectableMediaThumbnail(
            media = mediaItem,
            repository = repository,
            trashed = false,
            selected = selected,
            modifier = Modifier
                .aspectRatio(1f)
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .clickable { select(mediaItem) },
        )
    }
}

@Composable
internal fun <GridEntry> LazyMediaGrid(
    entries: List<GridEntry>,
    entryKey: (GridEntry) -> Any,
    entrySelected: (GridEntry) -> Boolean,
    contentPadding: PaddingValues,
    headerContent: (@Composable () -> Unit)?,
    footerContent: (@Composable () -> Unit)?,
    modifier: Modifier,
    entryContent: @Composable (GridEntry, Boolean) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(MINIMUM_MEDIA_CELL_SIZE),
        modifier = modifier,
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(1.dp),
        verticalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        headerContent?.let { header ->
            item(
                key = MEDIA_GRID_HEADER_KEY,
                span = { GridItemSpan(maxLineSpan) },
            ) {
                header()
            }
        }
        items(entries, key = entryKey) { gridEntry ->
            entryContent(gridEntry, entrySelected(gridEntry))
        }
        footerContent?.let { footer ->
            item(
                key = MEDIA_GRID_FOOTER_KEY,
                span = { GridItemSpan(maxLineSpan) },
            ) {
                footer()
            }
        }
    }
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

private val MINIMUM_MEDIA_CELL_SIZE = 112.dp
private const val MEDIA_GRID_HEADER_KEY = "media-grid-header"
private const val MEDIA_GRID_FOOTER_KEY = "media-grid-footer"
