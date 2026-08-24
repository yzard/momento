package io.github.yzard.momento.feature.map

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.graphics.drawable.toBitmap
import androidx.core.graphics.withClip
import androidx.core.view.doOnLayout
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.BoundingBox
import io.github.yzard.momento.core.model.MapCluster
import io.github.yzard.momento.core.model.Media
import coil.request.ImageRequest
import coil.request.SuccessResult
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.yield
import kotlinx.serialization.SerializationException
import org.osmdroid.config.Configuration
import org.osmdroid.events.MapListener
import org.osmdroid.events.ScrollEvent
import org.osmdroid.events.ZoomEvent
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.CustomZoomButtonsController
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.CopyrightOverlay
import org.osmdroid.views.overlay.Marker
import retrofit2.HttpException
import java.io.File
import java.io.IOException

fun initializeOpenStreetMap(context: Context) {
    val baseDirectory = File(context.cacheDir, "osmdroid")
    val tileDirectory = File(baseDirectory, "tiles")
    baseDirectory.mkdirs()
    tileDirectory.mkdirs()

    Configuration.getInstance().apply {
        load(context, context.getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        userAgentValue = "${context.packageName}/${BuildConfig.VERSION_NAME}"
        osmdroidBasePath = baseDirectory
        osmdroidTileCache = tileDirectory
    }
}

fun visibleMapBounds(north: Double, south: Double, east: Double, west: Double): BoundingBox? {
    val coordinates = listOf(north, south, east, west)
    if (coordinates.any { !it.isFinite() }) return null
    if (north !in -90.0..90.0 || south !in -90.0..90.0 || north <= south) return null
    if (east !in -180.0..180.0 || west !in -180.0..180.0) return null
    return BoundingBox(north, south, east, west)
}

fun mapViewport(
    north: Double,
    south: Double,
    east: Double,
    west: Double,
    zoom: Int,
): MapViewport? {
    val bounds = visibleMapBounds(north, south, east, west) ?: return null
    return MapViewport(bounds, zoom)
}

fun clusterClickZoom(currentZoom: Int, mediaCount: Long, maximumZoom: Int): Int {
    if (mediaCount <= 1) return currentZoom
    return (currentZoom + CLUSTER_ZOOM_STEP).coerceAtMost(maximumZoom)
}

private data class MapClusterSelection(
    val cluster: MapCluster,
    val bounds: BoundingBox,
)

data class MapViewport(
    val bounds: BoundingBox,
    val zoom: Int,
)

private data class OwnedMapView(
    val view: MapView,
    val listener: MapListener,
)

@OptIn(FlowPreview::class)
@Composable
fun NativeMapScreen(repository: MomentoRepository, showMedia: (List<Media>) -> Unit) {
    var error by remember { mutableStateOf<String?>(null) }
    val context = LocalContext.current
    val viewportRequests = remember { Channel<MapViewport>(Channel.CONFLATED) }
    val clusterSelections = remember { Channel<MapClusterSelection>(Channel.CONFLATED) }
    val currentShowMedia by rememberUpdatedState(showMedia)
    val lifecycleOwner = LocalLifecycleOwner.current
    val ownedMapView = remember(context) {
        val view = MapView(context).apply {
            setTileSource(TileSourceFactory.MAPNIK)
            setMultiTouchControls(true)
            zoomController.setVisibility(CustomZoomButtonsController.Visibility.NEVER)
            isVerticalMapRepetitionEnabled = false
            setScrollableAreaLimitLatitude(
                MapView.getTileSystem().maxLatitude,
                MapView.getTileSystem().minLatitude,
                0,
            )
            minZoomLevel = MINIMUM_MAP_ZOOM.toDouble()
            maxZoomLevel = MAXIMUM_MAP_ZOOM.toDouble()
            controller.setZoom(INITIAL_MAP_ZOOM.toDouble())
            controller.setCenter(GeoPoint(20.0, 0.0))
            overlays.add(
                CopyrightOverlay(context).apply {
                    setAlignRight(true)
                    setOffset(8, 12)
                },
            )
        }
        val listener = object : MapListener {
            override fun onScroll(event: ScrollEvent): Boolean {
                view.viewport()?.let(viewportRequests::trySend)
                return false
            }

            override fun onZoom(event: ZoomEvent): Boolean {
                view.viewport()?.let(viewportRequests::trySend)
                return false
            }
        }
        view.addMapListener(listener)
        view.doOnLayout { view.viewport()?.let(viewportRequests::trySend) }
        OwnedMapView(view, listener)
    }
    val mapView = ownedMapView.view

    LaunchedEffect(mapView) {
        repeat(INITIAL_VIEWPORT_RETRIES) {
            val viewport = mapView.viewport()
            if (viewport != null) {
                viewportRequests.send(viewport)
                return@LaunchedEffect
            }
            delay(INITIAL_VIEWPORT_RETRY_DELAY_MS)
        }
    }

    LaunchedEffect(repository, mapView) {
        viewportRequests.receiveAsFlow().debounce(300).collectLatest { viewport ->
            try {
                val clusters = repository.mapClusters(viewport.bounds, viewport.zoom).clusters
                val clusterMarkers = mutableListOf<Pair<MapCluster, Marker>>()
                clusters.chunked(MARKER_RENDER_BATCH_SIZE).forEach { clusterBatch ->
                    clusterBatch.forEach { cluster ->
                        val marker = Marker(mapView).apply {
                            position = GeoPoint(cluster.lat, cluster.lng)
                            title = "${cluster.count} photos"
                            icon = clusterMarkerDrawable(context, null, cluster.count)
                            setAnchor(CLUSTER_MARKER_ANCHOR_X, CLUSTER_MARKER_ANCHOR_Y)
                            setOnMarkerClickListener { _, _ ->
                                val nextZoom = clusterClickZoom(
                                    currentZoom = mapView.zoomLevelDouble.toInt(),
                                    mediaCount = cluster.count,
                                    maximumZoom = MAXIMUM_MAP_ZOOM,
                                )
                                if (nextZoom > mapView.zoomLevelDouble.toInt()) {
                                    mapView.controller.setCenter(position)
                                    mapView.controller.setZoom(nextZoom.toDouble())
                                }
                                clusterSelections.trySend(MapClusterSelection(cluster, viewport.bounds))
                                true
                            }
                        }
                        clusterMarkers += cluster to marker
                    }
                    yield()
                }
                mapView.overlays.removeAll { it is Marker }
                mapView.overlays.addAll(clusterMarkers.map { it.second })
                mapView.invalidate()
                error = null

                clusterMarkers.chunked(THUMBNAIL_LOAD_BATCH_SIZE).forEach { markerBatch ->
                    coroutineScope {
                        markerBatch.map { (cluster, marker) ->
                            async {
                                val thumbnail = loadClusterThumbnail(context, repository, cluster.representativeId)
                                marker.icon = clusterMarkerDrawable(context, thumbnail, cluster.count)
                            }
                        }.awaitAll()
                    }
                    mapView.invalidate()
                    yield()
                }
            } catch (_: IOException) {
                error = "Could not load map photos"
            } catch (_: HttpException) {
                error = "Could not load map photos"
            } catch (_: SerializationException) {
                error = "The server returned invalid map data"
            }
        }
    }

    LaunchedEffect(repository) {
        clusterSelections.receiveAsFlow().collectLatest { selection ->
            try {
                val media = repository.mapMedia(selection.bounds, clusterPrefixes(selection.cluster.id))
                if (media.isNotEmpty()) currentShowMedia(media)
                error = null
            } catch (_: IOException) {
                error = "Could not load photos from this area"
            } catch (_: HttpException) {
                error = "Could not load photos from this area"
            } catch (_: SerializationException) {
                error = "The server returned invalid map data"
            }
        }
    }

    Box(Modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { mapView },
            onRelease = { releasedView ->
                releasedView.removeMapListener(ownedMapView.listener)
                releasedView.overlays.clear()
                releasedView.onDetach()
            },
        )

        error?.let { message ->
            Surface(
                modifier = Modifier.align(Alignment.TopCenter).padding(12.dp),
                color = MaterialTheme.colorScheme.errorContainer,
                shape = MaterialTheme.shapes.large,
                shadowElevation = 4.dp,
            ) {
                TextButton(onClick = { error = null; mapView.viewport()?.let(viewportRequests::trySend) }) {
                    Text("$message. Retry")
                }
            }
        }
    }

    DisposableEffect(lifecycleOwner, mapView) {
        if (lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) {
            mapView.onResume()
        }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> mapView.onResume()
                Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            mapView.onPause()
        }
    }
}

private fun MapView.viewport(): MapViewport? {
    val box = boundingBox
    return mapViewport(box.latNorth, box.latSouth, box.lonEast, box.lonWest, zoomLevelDouble.toInt())
}

fun clusterPrefixes(clusterId: String): List<String> = listOf(clusterId)

private suspend fun loadClusterThumbnail(
    context: Context,
    repository: MomentoRepository,
    representativeId: Long,
): Bitmap? {
    val request = ImageRequest.Builder(context)
        .data(repository.thumbnailUrl(representativeId, true))
        .size(CLUSTER_THUMBNAIL_REQUEST_PX, CLUSTER_THUMBNAIL_REQUEST_PX)
        .allowHardware(false)
        .build()
    val result = repository.authenticatedImageLoader(context).execute(request) as? SuccessResult ?: return null
    val width = result.drawable.intrinsicWidth.coerceAtLeast(1)
    val height = result.drawable.intrinsicHeight.coerceAtLeast(1)
    return result.drawable.toBitmap(
        width,
        height,
        Bitmap.Config.ARGB_8888,
    )
}

private fun clusterMarkerDrawable(context: Context, thumbnail: Bitmap?, mediaCount: Long): Drawable =
    ClusterMarkerDrawable(context.resources.displayMetrics.density, thumbnail, mediaCount)

private class ClusterMarkerDrawable(
    private val density: Float,
    private val thumbnail: Bitmap?,
    private val mediaCount: Long,
) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    override fun draw(canvas: Canvas) {
        val width = CLUSTER_MARKER_WIDTH_DP * density
        val thumbnailTop = CLUSTER_THUMBNAIL_TOP_DP * density
        val thumbnailSize = CLUSTER_THUMBNAIL_SIZE_DP * density
        val thumbnailBounds = RectF(
            bounds.left.toFloat(),
            bounds.top + thumbnailTop,
            bounds.left + thumbnailSize,
            bounds.top + thumbnailTop + thumbnailSize,
        )
        val cornerRadius = CLUSTER_THUMBNAIL_CORNER_DP * density

        paint.color = Color.rgb(241, 245, 249)
        paint.setShadowLayer(6f * density, 0f, 3f * density, Color.argb(80, 15, 23, 42))
        canvas.drawRoundRect(thumbnailBounds, cornerRadius, cornerRadius, paint)
        paint.clearShadowLayer()
        if (thumbnail != null) {
            val clip = Path().apply {
                addRoundRect(thumbnailBounds, cornerRadius, cornerRadius, Path.Direction.CW)
            }
            val source = centerCropBounds(thumbnail.width, thumbnail.height)
            canvas.withClip(clip) {
                drawBitmap(
                    thumbnail,
                    Rect(source.left, source.top, source.right, source.bottom),
                    thumbnailBounds,
                    paint,
                )
            }
        }
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 2f * density
        paint.color = Color.WHITE
        canvas.drawRoundRect(thumbnailBounds, cornerRadius, cornerRadius, paint)
        paint.style = Paint.Style.FILL

        if (mediaCount > 1) {
            val label = mediaCount.toString()
            paint.typeface = Typeface.DEFAULT_BOLD
            paint.textSize = 12f * density
            paint.textAlign = Paint.Align.CENTER
            val badgeHeight = 26f * density
            val badgeWidth = maxOf(badgeHeight, paint.measureText(label) + 12f * density)
            val badgeBounds = RectF(
                bounds.left + width - badgeWidth,
                bounds.top.toFloat(),
                bounds.left + width,
                bounds.top + badgeHeight,
            )
            paint.color = Color.rgb(17, 24, 39)
            canvas.drawRoundRect(badgeBounds, badgeHeight / 2, badgeHeight / 2, paint)
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = 2f * density
            paint.color = Color.WHITE
            canvas.drawRoundRect(badgeBounds, badgeHeight / 2, badgeHeight / 2, paint)
            paint.style = Paint.Style.FILL
            paint.color = Color.WHITE
            val textY = badgeBounds.centerY() - (paint.ascent() + paint.descent()) / 2
            canvas.drawText(label, badgeBounds.centerX(), textY, paint)
        }
    }

    override fun setAlpha(alpha: Int) {
        paint.alpha = alpha
    }

    override fun setColorFilter(colorFilter: ColorFilter?) {
        paint.colorFilter = colorFilter
    }

    @Deprecated("Deprecated in Android")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT

    override fun getIntrinsicWidth(): Int = (CLUSTER_MARKER_WIDTH_DP * density).toInt()

    override fun getIntrinsicHeight(): Int = (CLUSTER_MARKER_HEIGHT_DP * density).toInt()
}

data class CropBounds(val left: Int, val top: Int, val right: Int, val bottom: Int)

fun centerCropBounds(width: Int, height: Int): CropBounds {
    require(width > 0 && height > 0) { "Image dimensions must be positive" }
    if (width == height) return CropBounds(0, 0, width, height)
    if (width > height) {
        val left = (width - height) / 2
        return CropBounds(left, 0, left + height, height)
    }
    val top = (height - width) / 2
    return CropBounds(0, top, width, top + width)
}

private const val MARKER_RENDER_BATCH_SIZE = 50
private const val THUMBNAIL_LOAD_BATCH_SIZE = 12
private const val MINIMUM_MAP_ZOOM = 2
private const val INITIAL_MAP_ZOOM = 3
private const val MAXIMUM_MAP_ZOOM = 20
private const val CLUSTER_ZOOM_STEP = 2
private const val CLUSTER_MARKER_WIDTH_DP = 68f
private const val CLUSTER_MARKER_HEIGHT_DP = 60f
private const val CLUSTER_THUMBNAIL_SIZE_DP = 52f
private const val CLUSTER_THUMBNAIL_REQUEST_PX = 96
private const val CLUSTER_THUMBNAIL_TOP_DP = 8f
private const val CLUSTER_THUMBNAIL_CORNER_DP = 10f
private const val CLUSTER_MARKER_ANCHOR_X = 26f / CLUSTER_MARKER_WIDTH_DP
private const val CLUSTER_MARKER_ANCHOR_Y = 34f / CLUSTER_MARKER_HEIGHT_DP
private const val INITIAL_VIEWPORT_RETRIES = 20
private const val INITIAL_VIEWPORT_RETRY_DELAY_MS = 50L
