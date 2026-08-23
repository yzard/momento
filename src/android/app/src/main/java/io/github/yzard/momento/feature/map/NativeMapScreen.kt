package io.github.yzard.momento.feature.map

import android.content.Context
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.doOnLayout
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.BoundingBox
import io.github.yzard.momento.core.model.MapCluster
import io.github.yzard.momento.core.model.Media
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.serialization.SerializationException
import org.osmdroid.config.Configuration
import org.osmdroid.events.MapListener
import org.osmdroid.events.ScrollEvent
import org.osmdroid.events.ZoomEvent
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
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

private data class MapClusterSnapshot(
    val clusters: List<MapCluster>,
    val bounds: BoundingBox,
)

private data class MapClusterSelection(
    val cluster: MapCluster,
    val bounds: BoundingBox,
)

@OptIn(FlowPreview::class)
@Composable
fun NativeMapScreen(repository: MomentoRepository, showMedia: (List<Media>) -> Unit) {
    var mapView by remember { mutableStateOf<MapView?>(null) }
    var clusterSnapshot by remember { mutableStateOf<MapClusterSnapshot?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val refreshRequests = remember { Channel<Unit>(Channel.CONFLATED) }
    val clusterSelections = remember { Channel<MapClusterSelection>(Channel.CONFLATED) }
    val currentShowMedia by rememberUpdatedState(showMedia)
    val lifecycleOwner = LocalLifecycleOwner.current

    LaunchedEffect(mapView) {
        val view = mapView ?: return@LaunchedEffect
        refreshRequests.receiveAsFlow().debounce(300).collectLatest {
            val box = view.boundingBox
            val bounds = visibleMapBounds(box.latNorth, box.latSouth, box.lonEast, box.lonWest)
                ?: return@collectLatest
            try {
                val clusters = repository.mapClusters(bounds, view.zoomLevelDouble.toInt()).clusters
                clusterSnapshot = MapClusterSnapshot(clusters, bounds)
                error = null
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

    LaunchedEffect(mapView, clusterSnapshot) {
        val view = mapView ?: return@LaunchedEffect
        view.overlays.removeAll { it is Marker }
        val snapshot = clusterSnapshot ?: return@LaunchedEffect
        snapshot.clusters.forEach { cluster ->
            view.overlays.add(
                Marker(view).apply {
                    position = GeoPoint(cluster.lat, cluster.lng)
                    title = "${cluster.count} photos"
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                    setOnMarkerClickListener { _, _ ->
                        clusterSelections.trySend(MapClusterSelection(cluster, snapshot.bounds))
                        true
                    }
                },
            )
        }
        view.invalidate()
    }

    Box(Modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                MapView(context).apply {
                    setTileSource(TileSourceFactory.MAPNIK)
                    setMultiTouchControls(true)
                    minZoomLevel = 2.0
                    maxZoomLevel = 20.0
                    controller.setZoom(2.0)
                    controller.setCenter(GeoPoint(20.0, 0.0))
                    addMapListener(
                        object : MapListener {
                            override fun onScroll(event: ScrollEvent): Boolean {
                                refreshRequests.trySend(Unit)
                                return false
                            }

                            override fun onZoom(event: ZoomEvent): Boolean {
                                refreshRequests.trySend(Unit)
                                return false
                            }
                        },
                    )
                    doOnLayout { refreshRequests.trySend(Unit) }
                    mapView = this
                }
            },
        )

        Surface(
            modifier = Modifier.align(Alignment.BottomStart).padding(8.dp),
            color = MaterialTheme.colorScheme.surface.copy(alpha = 0.88f),
            shape = MaterialTheme.shapes.small,
        ) {
            Text(
                text = "OpenStreetMap contributors",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            )
        }

        error?.let { message ->
            Surface(
                modifier = Modifier.align(Alignment.TopCenter).padding(12.dp),
                color = MaterialTheme.colorScheme.errorContainer,
                shape = MaterialTheme.shapes.large,
                shadowElevation = 4.dp,
            ) {
                TextButton(onClick = { error = null; refreshRequests.trySend(Unit) }) {
                    Text("$message. Retry")
                }
            }
        }
    }

    DisposableEffect(lifecycleOwner, mapView) {
        val view = mapView
        if (view != null && lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) {
            view.onResume()
        }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> view?.onResume()
                Lifecycle.Event.ON_PAUSE -> view?.onPause()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            view?.onPause()
            view?.onDetach()
        }
    }
}

fun clusterPrefixes(clusterId: String): List<String> = listOf(clusterId)
