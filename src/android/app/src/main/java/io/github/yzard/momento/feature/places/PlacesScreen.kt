package io.github.yzard.momento.feature.places

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.app.navigation.LibraryChange
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.RequestResult
import io.github.yzard.momento.core.data.runRequest
import io.github.yzard.momento.core.data.userMessage
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.Place
import io.github.yzard.momento.core.ui.MemoryCardOverlay
import io.github.yzard.momento.core.ui.MomentoAsyncImage
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import io.github.yzard.momento.feature.media.MomentoCollectionDetail
import io.github.yzard.momento.feature.media.PageState
import io.github.yzard.momento.feature.media.beginCursorPage
import io.github.yzard.momento.feature.media.completeCursorPage
import io.github.yzard.momento.feature.media.emptyCursorPagingState
import io.github.yzard.momento.feature.media.failCursorPage
import io.github.yzard.momento.feature.media.shouldLoadMoreMedia
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.filter

fun placeGridColumns(widthDp: Int): Int = when {
    widthDp < 360 -> 1
    widthDp < 600 -> 2
    widthDp < 840 -> 3
    widthDp < 1200 -> 4
    else -> 5
}

fun placeRegion(place: Place): String = listOfNotNull(place.state, place.country).joinToString(", ")

fun placeDetailSubtitle(place: Place): String =
    listOf(placeRegion(place), "${place.mediaCount} media").filter { it.isNotEmpty() }.joinToString(" · ")

@Composable
fun PlacesScreen(
    repository: MomentoRepository,
    libraryChange: LibraryChange?,
    openPlace: (Place) -> Unit,
) {
    var pagingState by remember(repository) { mutableStateOf(emptyCursorPagingState<Place>()) }
    val scope = rememberCoroutineScope()

    suspend fun loadPlaces(reset: Boolean) {
        val loadingState = beginCursorPage(pagingState, reset) ?: return
        pagingState = loadingState
        when (val requestResult = runRequest { repository.places(if (reset) null else loadingState.nextCursor) }) {
            is RequestResult.Success -> pagingState = completeCursorPage(
                state = loadingState,
                page = requestResult.response.places,
                nextCursor = requestResult.response.nextCursor,
                hasMore = requestResult.response.hasMore,
                key = Place::placeId,
            )
            is RequestResult.Failure -> pagingState = failCursorPage(
                loadingState,
                requestResult.error.userMessage("Could not load places"),
            )
        }
    }

    LaunchedEffect(repository, libraryChange?.sequence) { loadPlaces(true) }
    MomentoPageScaffold(
        title = "Places",
        subtitle = null,
        backContentDescription = null,
        onBack = null,
        trailingContent = null,
        reserveBottomControls = true,
        edgeToEdgeContent = false,
        bottomContent = null,
        modifier = Modifier,
    ) { contentPadding ->
        when {
            !pagingState.initialized && pagingState.error != null -> ErrorState(
                requireNotNull(pagingState.error),
                { scope.launch { loadPlaces(true) } },
                Modifier,
            )
            !pagingState.initialized -> LoadingState("Loading places", Modifier)
            pagingState.entries.isEmpty() -> EmptyState(
                "No places yet",
                "Places will appear when memories contain location information.",
                Modifier,
            )
            else -> PlaceTiles(
                places = pagingState.entries,
                repository = repository,
                hasMore = pagingState.hasMore,
                loading = pagingState.loading,
                contentPadding = contentPadding,
                loadMore = { scope.launch { loadPlaces(false) } },
                select = openPlace,
            )
        }
    }
}

@Composable
private fun PlaceTiles(
    places: List<Place>,
    repository: MomentoRepository,
    hasMore: Boolean,
    loading: Boolean,
    contentPadding: PaddingValues,
    loadMore: () -> Unit,
    select: (Place) -> Unit,
) {
    val gridState = rememberLazyGridState()
    LaunchedEffect(gridState, hasMore, loading) {
        snapshotFlow {
            val layout = gridState.layoutInfo
            shouldLoadMoreMedia(
                lastVisibleItemIndex = layout.visibleItemsInfo.lastOrNull()?.index ?: -1,
                totalItemsCount = layout.totalItemsCount,
                hasMore = hasMore,
                loading = loading,
            )
        }.filter { it }.collect { loadMore() }
    }
    BoxWithConstraints(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        val columns = placeGridColumns(maxWidth.value.toInt())
        LazyVerticalGrid(
            columns = GridCells.Fixed(columns),
            state = gridState,
            contentPadding = contentPadding,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            items(places, key = { it.placeId }) { place ->
                PlaceTile(place, repository) { select(place) }
            }
            if (hasMore) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Text(
                        text = if (loading) "Loading more..." else "Load more",
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable(enabled = !loading, onClick = loadMore)
                            .padding(16.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun PlaceTile(place: Place, repository: MomentoRepository, select: () -> Unit) {
    val thumbnail by produceState<ByteArray?>(null, place.placeId) {
        value = when (val requestResult = runRequest { repository.placeThumbnail(place.placeId) }) {
            is RequestResult.Success -> requestResult.response
            is RequestResult.Failure -> null
        }
    }
    val shape = RoundedCornerShape(16.dp)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(3f / 2f)
            .clip(shape)
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, shape)
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .semantics {
                contentDescription = "${place.city}, ${placeRegion(place)}, ${place.mediaCount} media"
            }
            .clickable(onClick = select),
    ) {
        if (thumbnail != null) {
            MomentoAsyncImage(
                model = thumbnail,
                repository = repository,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            Icon(
                imageVector = Icons.Default.LocationOn,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.45f),
                modifier = Modifier.align(Alignment.Center),
            )
        }
        MemoryCardOverlay(
            title = place.city,
            subtitle = placeRegion(place),
            badge = "${place.mediaCount} media",
        )
    }
}

@Composable
internal fun PlaceDetailScreen(
    repository: MomentoRepository,
    place: Place,
    close: () -> Unit,
    libraryChange: LibraryChange?,
    openMedia: (List<Media>, Int) -> Unit,
) {
    val placeId = place.placeId
    var pagingState by remember(placeId) { mutableStateOf(emptyCursorPagingState<Media>()) }
    val scope = rememberCoroutineScope()

    suspend fun loadPlace(reset: Boolean) {
        val loadingState = beginCursorPage(pagingState, reset) ?: return
        pagingState = loadingState
        when (val requestResult = runRequest { repository.place(placeId, if (reset) null else loadingState.nextCursor) }) {
            is RequestResult.Success -> pagingState = completeCursorPage(
                state = loadingState,
                page = requestResult.response.media,
                nextCursor = requestResult.response.nextCursor,
                hasMore = requestResult.response.hasMore,
                key = Media::id,
            )
            is RequestResult.Failure -> pagingState = failCursorPage(
                loadingState,
                requestResult.error.userMessage("Could not load this place"),
            )
        }
    }

    LaunchedEffect(placeId, libraryChange?.sequence) { loadPlace(reset = true) }

    val pageState: PageState<List<Media>> = when {
        !pagingState.initialized && pagingState.error != null ->
            PageState.Failed(requireNotNull(pagingState.error))
        !pagingState.initialized -> PageState.Loading
        else -> PageState.Ready(pagingState.entries, refreshing = false)
    }

    MomentoCollectionDetail(
        title = place.city,
        subtitle = placeDetailSubtitle(place),
        backContentDescription = "Back to places",
        repository = repository,
        pageState = pageState,
        selectedMediaIds = emptySet(),
        reserveBottomControls = false,
        bottomContent = null,
        footerContent = if (pagingState.hasMore && pagingState.nextCursor != null) {
            {
                Text(
                    if (pagingState.loading) "Loading more..." else if (pagingState.error == null) "Load more" else "Retry loading more",
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(enabled = !pagingState.loading) {
                            scope.launch { loadPlace(reset = false) }
                        }
                        .padding(16.dp),
                )
            }
        } else {
            null
        },
        contentError = null,
        loadingLabel = "Loading place",
        emptyTitle = "No media",
        emptyExplanation = "No visible media is assigned to this place.",
        close = close,
        retry = { scope.launch { loadPlace(reset = true) } },
        select = { mediaItem, media -> openMedia(media, media.indexOf(mediaItem)) },
    )
}
