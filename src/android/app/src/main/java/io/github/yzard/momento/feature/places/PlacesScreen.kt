package io.github.yzard.momento.feature.places

import android.content.res.Configuration
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import coil.compose.AsyncImage
import coil.request.ImageRequest
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.Place
import io.github.yzard.momento.feature.media.EmptyState
import io.github.yzard.momento.feature.media.ErrorState
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.media.MediaGrid
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException
import java.util.Base64

fun placeGridColumns(isPortrait: Boolean, widthDp: Int): Int = when {
    isPortrait -> 2
    widthDp >= 900 -> 4
    else -> 3
}

fun decodePlaceThumbnail(dataUrl: String?): ByteArray? {
    if (dataUrl == null) return null
    val separator = dataUrl.indexOf(',')
    if (separator <= 0 || !dataUrl.substring(0, separator).endsWith(";base64")) return null
    return try {
        Base64.getDecoder().decode(dataUrl.substring(separator + 1))
    } catch (_: IllegalArgumentException) {
        null
    }
}

fun placeRegion(place: Place): String = listOfNotNull(place.state, place.country).joinToString(", ")

@Composable
fun PlacesScreen(repository: MomentoRepository, openMedia: (List<Media>, Int) -> Unit) {
    var places by remember(repository) { mutableStateOf<List<Place>?>(null) }
    var nextCursor by remember(repository) { mutableStateOf<String?>(null) }
    var hasMore by remember(repository) { mutableStateOf(false) }
    var loading by remember(repository) { mutableStateOf(false) }
    var selected by remember(repository) { mutableStateOf<Place?>(null) }
    var error by remember(repository) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    suspend fun loadPlaces(reset: Boolean) {
        if (loading || (!reset && (!hasMore || nextCursor == null))) return
        loading = true
        if (reset) {
            places = null
            nextCursor = null
            hasMore = false
        }
        try {
            val response = repository.places(if (reset) null else nextCursor)
            places = if (reset) response.places else appendPlaces(places.orEmpty(), response.places)
            nextCursor = response.nextCursor
            hasMore = response.hasMore
            error = null
        } catch (_: IOException) {
            error = "Could not load places"
        } catch (_: HttpException) {
            error = "Could not load places"
        } catch (_: SerializationException) {
            error = "Could not load places"
        } finally {
            loading = false
        }
    }

    LaunchedEffect(repository) { loadPlaces(true) }
    BackHandler(enabled = selected != null) { selected = null }

    val selectedPlace = selected
    if (selectedPlace != null) {
        PlaceDetailScreen(repository, selectedPlace.placeId, openMedia)
        return
    }

    when {
        places == null && error != null -> ErrorState(error!!) { scope.launch { loadPlaces(true) } }
        places == null -> LoadingState()
        places!!.isEmpty() -> EmptyState("No places yet")
        else -> PlaceTiles(
            places = places!!,
            repository = repository,
            hasMore = hasMore,
            loading = loading,
            loadMore = { scope.launch { loadPlaces(false) } },
            select = { selected = it },
        )
    }
}

@Composable
private fun PlaceTiles(
    places: List<Place>,
    repository: MomentoRepository,
    hasMore: Boolean,
    loading: Boolean,
    loadMore: () -> Unit,
    select: (Place) -> Unit,
) {
    val configuration = LocalConfiguration.current
    BoxWithConstraints(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        val columns = placeGridColumns(
            isPortrait = configuration.orientation == Configuration.ORIENTATION_PORTRAIT,
            widthDp = maxWidth.value.toInt(),
        )
        LazyVerticalGrid(
            columns = GridCells.Fixed(columns),
            contentPadding = PaddingValues(start = 10.dp, top = 72.dp, end = 10.dp, bottom = 92.dp),
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
    val context = LocalContext.current
    val thumbnail by produceState<ByteArray?>(null, place.placeId) {
        value = try {
            decodePlaceThumbnail(repository.placeThumbnail(place.placeId))
        } catch (_: IOException) {
            null
        } catch (_: HttpException) {
            null
        } catch (_: SerializationException) {
            null
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
            AsyncImage(
                model = ImageRequest.Builder(context).data(thumbnail).build(),
                imageLoader = repository.authenticatedImageLoader(context),
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
        Box(
            Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colorStops = arrayOf(
                            0.35f to Color.Transparent,
                            0.68f to Color.Black.copy(alpha = 0.58f),
                            1f to Color.Black.copy(alpha = 0.92f),
                        ),
                    ),
                ),
        )
        Row(
            modifier = Modifier.align(Alignment.BottomStart).fillMaxWidth().padding(12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.Bottom,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = place.city,
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = placeRegion(place),
                    color = Color.White.copy(alpha = 0.8f),
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Surface(
                color = Color.Black.copy(alpha = 0.48f),
                contentColor = Color.White,
                shape = RoundedCornerShape(20.dp),
                modifier = Modifier.padding(start = 8.dp),
            ) {
                Text(
                    text = "${place.mediaCount} media",
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                )
            }
        }
    }
}

@Composable
private fun PlaceDetailScreen(
    repository: MomentoRepository,
    placeId: String,
    openMedia: (List<Media>, Int) -> Unit,
) {
    var media by remember(placeId) { mutableStateOf<List<Media>?>(null) }
    var nextCursor by remember(placeId) { mutableStateOf<String?>(null) }
    var more by remember(placeId) { mutableStateOf(true) }
    var requestCursor by remember(placeId) { mutableStateOf<String?>(null) }
    var error by remember(placeId) { mutableStateOf<String?>(null) }
    var loading by remember(placeId) { mutableStateOf(false) }
    var retryVersion by remember(placeId) { mutableIntStateOf(0) }

    LaunchedEffect(placeId, requestCursor, retryVersion) {
        loading = true
        try {
            val response = repository.place(placeId, requestCursor)
            media = appendPlaceMedia(media.orEmpty(), response.media)
            nextCursor = response.nextCursor
            more = response.hasMore
            error = null
        } catch (_: IOException) {
            error = "Could not load this place"
        } catch (_: HttpException) {
            error = "Could not load this place"
        } catch (_: SerializationException) {
            error = "Could not load this place"
        } finally {
            loading = false
        }
    }

    when {
        media == null && error != null -> ErrorState(error!!) { retryVersion += 1 }
        media == null -> LoadingState()
        else -> MediaGrid(
            media = media!!,
            repository = repository,
            selectedMediaIds = emptySet(),
            contentPadding = PaddingValues(top = 64.dp, bottom = 88.dp),
            headerContent = null,
            footerContent = if (more && nextCursor != null) {
                {
                    Text(
                        if (loading) "Loading more..." else if (error == null) "Load more" else "Retry loading more",
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable(enabled = !loading) {
                                if (error == null) requestCursor = nextCursor else retryVersion += 1
                            }
                            .padding(16.dp),
                    )
                }
            } else {
                null
            },
            modifier = Modifier.fillMaxSize(),
        ) { mediaItem ->
            openMedia(media!!, media!!.indexOf(mediaItem))
        }
    }
}

fun appendPlaceMedia(existing: List<Media>, page: List<Media>): List<Media> =
    existing + page.filter { candidate -> existing.none { it.id == candidate.id } }

fun appendPlaces(existing: List<Place>, page: List<Place>): List<Place> =
    existing + page.filter { candidate -> existing.none { it.placeId == candidate.placeId } }
