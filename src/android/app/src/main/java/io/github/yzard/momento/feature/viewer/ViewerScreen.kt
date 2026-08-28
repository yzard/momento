package io.github.yzard.momento.feature.viewer

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.res.Configuration
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.VolumeOff
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.AddPhotoAlternate
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.MediaItem as PlayerMediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.PlayerView
import coil.compose.AsyncImage
import coil.request.ImageRequest
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.feature.albums.AlbumAddMediaSheet
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.app.designsystem.LocalMomentoDarkTheme
import io.github.yzard.momento.feature.media.MediaThumbnail
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.File
import java.io.IOException
import java.time.DateTimeException
import java.time.Instant
import java.time.LocalDateTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.abs
import androidx.compose.runtime.CompositionLocalProvider

data class ViewerTimestamp(val date: String, val time: String)
data class FilmstripItemBounds(val index: Int, val offset: Int, val size: Int)
data class FilmstripEdgeVisibility(val left: Boolean, val right: Boolean)
enum class ViewerInformationPresentation { BOTTOM, RIGHT }

fun viewerIndex(index: Int, change: Int, size: Int): Int {
    if (size <= 0) return 0
    return (index + change).coerceIn(0, size - 1)
}

fun removeViewedMedia(media: List<Media>, index: Int): Pair<List<Media>, Int> {
    val remaining = media.filterIndexed { itemIndex, _ -> itemIndex != index }
    return remaining to index.coerceAtMost(remaining.lastIndex.coerceAtLeast(0))
}

fun viewerTimestamp(media: Media): ViewerTimestamp {
    val dateTime = parseMediaDateTime(media.dateTaken ?: media.createdAt)
        ?: return ViewerTimestamp("Unknown", "")
    return ViewerTimestamp(
        date = dateTime.format(DateTimeFormatter.ofPattern("MMM dd", Locale.ENGLISH)),
        time = dateTime.format(DateTimeFormatter.ofPattern("hh:mm a", Locale.ENGLISH)),
    )
}

fun mediaMetadataRows(media: Media): List<Pair<String, String>> = listOfNotNull(
    "Filename" to media.originalFilename,
    "Type" to media.mediaType.replaceFirstChar { it.uppercase() },
    media.mimeType?.let { "MIME type" to it },
    media.width?.let { width -> "Dimensions" to "$width x ${media.height ?: "?"}" },
    media.fileSize?.let { "File size" to formatFileSize(it) },
    media.durationSeconds?.let { "Duration" to formatDuration(it) },
    media.dateTaken?.let { "Captured" to it },
    listOfNotNull(media.cameraMake, media.cameraModel).takeIf { it.isNotEmpty() }?.let { "Camera" to it.joinToString(" ") },
    listOfNotNull(media.lensMake, media.lensModel).takeIf { it.isNotEmpty() }?.let { "Lens" to it.joinToString(" ") },
    media.iso?.let { "ISO" to it.toString() },
    media.exposureTime?.let { "Exposure" to it },
    media.fNumber?.let { "Aperture" to "f/$it" },
    media.focalLength?.let { "Focal length" to "$it mm" },
    media.focalLength35mm?.let { "35 mm equivalent" to "$it mm" },
    media.locationCity?.let { city ->
        "Location" to listOfNotNull(city, media.locationState, media.locationCountry).joinToString(", ")
    },
    media.gpsLatitude?.let { latitude ->
        media.gpsLongitude?.let { longitude -> "Coordinates" to "$latitude, $longitude" }
    },
    media.gpsAltitude?.let { "Altitude" to "$it m" },
    media.videoCodec?.let { "Video codec" to it },
    media.keywords?.takeIf { it.isNotBlank() }?.let { "Keywords" to it },
    media.contentHash?.takeIf { it.isNotBlank() }?.let { "Content hash" to it },
    "Created" to media.createdAt,
)

fun shareCacheFilename(media: Media): String {
    val safeName = media.originalFilename.replace(Regex("[^A-Za-z0-9._-]"), "_").ifBlank { media.filename }
    return "${media.id}-$safeName"
}

fun isExpiredShareCacheFile(lastModifiedMillis: Long, nowMillis: Long): Boolean =
    lastModifiedMillis <= 0 || nowMillis - lastModifiedMillis >= SHARE_CACHE_EXPIRY_MILLIS

fun boundedViewerPan(offset: Offset, viewportSize: IntSize, scale: Float): Offset {
    if (scale <= 1f || viewportSize.width <= 0 || viewportSize.height <= 0) return Offset.Zero
    val maximumX = viewportSize.width * (scale - 1f) / 2f
    val maximumY = viewportSize.height * (scale - 1f) / 2f
    return Offset(
        x = offset.x.coerceIn(-maximumX, maximumX),
        y = offset.y.coerceIn(-maximumY, maximumY),
    )
}

fun shouldToggleViewerChrome(maxPointerCount: Int, movedBeyondTouchSlop: Boolean): Boolean =
    maxPointerCount == 1 && !movedBeyondTouchSlop

fun boundedPlaybackPosition(positionMs: Float, durationMs: Long): Long {
    if (durationMs <= 0) return 0
    return positionMs.toLong().coerceIn(0, durationMs)
}

fun filmstripEdgeVisibility(
    canScrollBackward: Boolean,
    canScrollForward: Boolean,
): FilmstripEdgeVisibility = FilmstripEdgeVisibility(
    left = canScrollBackward,
    right = canScrollForward,
)

fun playbackProgressFraction(positionMs: Float, durationMs: Long): Float {
    if (durationMs <= 0) return 0f
    return (positionMs / durationMs.toFloat()).coerceIn(0f, 1f)
}

fun viewerInformationPresentation(landscape: Boolean): ViewerInformationPresentation =
    if (landscape) ViewerInformationPresentation.RIGHT else ViewerInformationPresentation.BOTTOM

fun centeredFilmstripIndex(
    viewportStartOffset: Int,
    viewportEndOffset: Int,
    items: List<FilmstripItemBounds>,
): Int? {
    if (items.isEmpty()) return null
    val viewportCenter = (viewportStartOffset + viewportEndOffset) / 2
    return items.minByOrNull { item -> abs(item.offset + item.size / 2 - viewportCenter) }?.index
}

@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
fun ViewerScreen(
    media: List<Media>,
    initialIndex: Int,
    repository: MomentoRepository,
    viewedIndexChanged: (Int) -> Unit,
    mediaChanged: (Long) -> Unit,
    close: () -> Unit,
) {
    var items by remember(media) { mutableStateOf(media) }
    val pagerState = rememberPagerState(
        initialPage = viewerIndex(initialIndex, 0, media.size),
        pageCount = { items.size },
    )
    var confirmTrash by remember { mutableStateOf(false) }
    var trashing by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var chromeState by rememberSaveable(
        stateSaver = Saver(
            save = { state -> state.restorationValues() },
            restore = ::restoreViewerChromeState,
        ),
    ) { mutableStateOf(ViewerChromeState.initial()) }
    var activeVideoPlayer by remember { mutableStateOf<Pair<Long, ExoPlayer>?>(null) }
    var zoomedMediaId by remember { mutableStateOf<Long?>(null) }
    var pendingShareFile by remember { mutableStateOf<File?>(null) }
    val context = LocalContext.current
    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    val informationPresentation = viewerInformationPresentation(landscape)
    val view = LocalView.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    val shareLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        pendingShareFile?.delete()
        pendingShareFile = null
        sharing = false
    }

    if (items.isEmpty()) {
        LaunchedEffect(Unit) { close() }
        return
    }

    val index = pagerState.currentPage.coerceIn(0, items.lastIndex)
    val item = items[index]
    val timestamp = remember(item.id, item.dateTaken, item.createdAt) { viewerTimestamp(item) }

    fun recordChromeInteraction() {
        chromeState = chromeState.recordInteraction()
    }

    fun changeChromeInteraction(active: Boolean) {
        chromeState = chromeState.changeInteraction(active)
    }

    LaunchedEffect(item.id) { zoomedMediaId = null }
    LaunchedEffect(index) { viewedIndexChanged(index) }
    LaunchedEffect(chromeState, item.id) {
        if (!chromeState.visible || chromeState.sheet != null || chromeState.interactionActive) return@LaunchedEffect
        delay(3_500)
        chromeState = chromeState.hideAfterInactivity()
    }
    DisposableEffect(view) {
        val window = (view.context as Activity).window
        val insetsController = WindowCompat.getInsetsController(window, view)
        insetsController.systemBarsBehavior =
            androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        insetsController.hide(WindowInsetsCompat.Type.systemBars())
        onDispose { insetsController.show(WindowInsetsCompat.Type.systemBars()) }
    }
    LaunchedEffect(context.cacheDir) {
        val shareDirectory = File(context.cacheDir, SHARE_CACHE_DIRECTORY)
        val nowMillis = System.currentTimeMillis()
        shareDirectory.listFiles()?.forEach { cachedFile ->
            if (isExpiredShareCacheFile(cachedFile.lastModified(), nowMillis)) cachedFile.delete()
        }
    }

    suspend fun shareCurrent() {
        if (sharing) return
        sharing = true
        var sharedFile: File? = null
        var launched = false
        try {
            sharedFile = File(File(context.cacheDir, SHARE_CACHE_DIRECTORY), shareCacheFilename(item))
            repository.downloadOriginal(item.id, requireNotNull(sharedFile))
            requireNotNull(sharedFile).setLastModified(System.currentTimeMillis())
            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                requireNotNull(sharedFile),
            )
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = item.mimeType ?: if (item.mediaType == "video") "video/*" else "image/*"
                putExtra(Intent.EXTRA_STREAM, uri)
                putExtra(Intent.EXTRA_TITLE, item.originalFilename)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            pendingShareFile = sharedFile
            shareLauncher.launch(Intent.createChooser(intent, "Share media"))
            launched = true
        } catch (_: IOException) {
            sharedFile?.delete()
            error = "Could not prepare this media for sharing"
        } catch (_: ActivityNotFoundException) {
            sharedFile?.delete()
            error = "No application is available to share this media"
        } finally {
            if (!launched) {
                pendingShareFile = null
                sharing = false
            }
        }
    }

    suspend fun trashCurrent() {
        if (trashing) return
        trashing = true
        confirmTrash = false
        try {
            repository.moveToTrash(listOf(item.id))
            val result = removeViewedMedia(items, index)
            items = result.first
            mediaChanged(item.id)
            if (result.first.isEmpty()) {
                close()
            } else {
                pagerState.scrollToPage(result.second)
            }
        } catch (_: IOException) {
            error = "Could not move this media to Trash"
        } catch (_: HttpException) {
            error = "Could not move this media to Trash"
        } catch (_: SerializationException) {
            error = "Could not move this media to Trash"
        } finally {
            trashing = false
        }
    }

    BackHandler {
        if (chromeState.sheet != null) {
            chromeState = chromeState.closeSheet()
        } else {
            close()
        }
    }

    CompositionLocalProvider(LocalMomentoDarkTheme provides true) {
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        HorizontalPager(
            state = pagerState,
            key = { page -> items[page].id },
            userScrollEnabled = chromeState.sheet == null && zoomedMediaId == null,
            modifier = Modifier.fillMaxSize(),
        ) { page ->
            ViewerMedia(
                media = items[page],
                repository = repository,
                active = page == index,
                toggleChrome = { chromeState = chromeState.toggle() },
                zoomChanged = { zoomed ->
                    val pageMediaId = items[page].id
                    if (zoomed && page == index) {
                        zoomedMediaId = pageMediaId
                    } else if (zoomedMediaId == pageMediaId) {
                        zoomedMediaId = null
                    }
                },
                playerChanged = { mediaId, player ->
                    if (player == null) {
                        if (activeVideoPlayer?.first == mediaId) activeVideoPlayer = null
                    } else {
                        activeVideoPlayer = mediaId to player
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
        }

        AnimatedVisibility(
            visible = chromeState.visible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.fillMaxWidth().align(Alignment.TopCenter),
        ) {
            ViewerTopControls(timestamp = timestamp, close = close, modifier = Modifier.fillMaxWidth())
        }
        AnimatedVisibility(
            visible = chromeState.visible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.fillMaxWidth().align(Alignment.BottomCenter),
        ) {
            ViewerBottomControls(
                media = items,
                currentIndex = index,
                repository = repository,
                player = activeVideoPlayer?.takeIf { it.first == item.id }?.second,
                navigate = { target -> scope.launch { pagerState.animateScrollToPage(target) } },
                sharing = sharing,
                share = { scope.launch { shareCurrent() } },
                albums = { chromeState = chromeState.openSheet(ViewerSheet.ALBUMS) },
                information = { chromeState = chromeState.openSheet(ViewerSheet.INFORMATION) },
                trash = { confirmTrash = true },
                userInteracted = ::recordChromeInteraction,
                interactionChanged = ::changeChromeInteraction,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
    }

    if (
        chromeState.sheet == ViewerSheet.ALBUMS ||
        (chromeState.sheet == ViewerSheet.INFORMATION && informationPresentation == ViewerInformationPresentation.BOTTOM)
    ) {
        ModalBottomSheet(
            onDismissRequest = { chromeState = chromeState.closeSheet() },
            sheetState = sheetState,
            containerColor = MaterialTheme.colorScheme.background,
            contentColor = MaterialTheme.colorScheme.onBackground,
        ) {
            when (chromeState.sheet) {
                ViewerSheet.ALBUMS -> AlbumAddMediaSheet(
                    repository = repository,
                    mediaIds = listOf(item.id),
                    close = { chromeState = chromeState.closeSheet() },
                )
                ViewerSheet.INFORMATION -> MediaInformationContent(item, Modifier.fillMaxWidth().fillMaxHeight(0.72f))
                null -> Unit
            }
        }
    }

    AnimatedVisibility(
        visible = chromeState.sheet == ViewerSheet.INFORMATION &&
            informationPresentation == ViewerInformationPresentation.RIGHT,
        enter = fadeIn() + slideInHorizontally(initialOffsetX = { fullWidth -> fullWidth }),
        exit = fadeOut() + slideOutHorizontally(targetOffsetX = { fullWidth -> fullWidth }),
        modifier = Modifier.fillMaxSize(),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.42f))
                .clickable { chromeState = chromeState.closeSheet() },
        ) {
            Surface(
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .fillMaxHeight()
                    .widthIn(max = 420.dp)
                    .fillMaxWidth(0.46f)
                    .clickable(onClick = {}),
                color = MaterialTheme.colorScheme.background,
                contentColor = MaterialTheme.colorScheme.onBackground,
            ) {
                MediaInformationContent(
                    media = item,
                    modifier = Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
                )
            }
        }
    }

    if (confirmTrash) {
        AlertDialog(
            onDismissRequest = { if (!trashing) confirmTrash = false },
            title = { Text("Move to Trash?") },
            text = { Text(item.originalFilename) },
            confirmButton = {
                TextButton(
                    onClick = { scope.launch { trashCurrent() } },
                    enabled = !trashing,
                ) { Text("Move") }
            },
            dismissButton = {
                TextButton(onClick = { confirmTrash = false }, enabled = !trashing) { Text("Cancel") }
            },
        )
    }

    error?.let { message ->
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text("Media action unavailable") },
            text = { Text(message) },
            confirmButton = { TextButton(onClick = { error = null }) { Text("OK") } },
        )
    }
}

@Composable
private fun ViewerMedia(
    media: Media,
    repository: MomentoRepository,
    active: Boolean,
    toggleChrome: () -> Unit,
    zoomChanged: (Boolean) -> Unit,
    playerChanged: (Long, ExoPlayer?) -> Unit,
    modifier: Modifier,
) {
    if (media.mediaType == "video") {
        LaunchedEffect(media.id, active) { if (active) zoomChanged(false) }
        VideoViewer(media, repository, active, toggleChrome, playerChanged, modifier)
        return
    }

    val context = LocalContext.current
    val url by produceState<String?>(null, media.id) { value = repository.previewUrl(media.id) }
    var scale by remember(media.id) { mutableFloatStateOf(1f) }
    var panOffset by remember(media.id) { mutableStateOf(Offset.Zero) }
    var viewportSize by remember(media.id) { mutableStateOf(IntSize.Zero) }
    LaunchedEffect(media.id, active) {
        if (!active) {
            scale = 1f
            panOffset = Offset.Zero
            zoomChanged(false)
        }
    }
    DisposableEffect(media.id) { onDispose { zoomChanged(false) } }
    AsyncImage(
        model = url?.let { ImageRequest.Builder(context).data(it).build() },
        imageLoader = repository.authenticatedImageLoader(context),
        contentDescription = media.originalFilename,
        contentScale = ContentScale.Fit,
        modifier = modifier
            .viewerChromeToggle(toggleChrome)
            .onSizeChanged { viewportSize = it }
            .pointerInput(media.id) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false)
                    var event: PointerEvent
                    do {
                        event = awaitPointerEvent()
                        val pointerCount = event.changes.count { it.pressed }
                        if (pointerCount >= 2 || scale > 1f) {
                            val nextScale = if (pointerCount >= 2) {
                                (scale * event.calculateZoom()).coerceIn(1f, 5f)
                            } else {
                                scale
                            }
                            panOffset = if (nextScale <= 1f) {
                                Offset.Zero
                            } else {
                                boundedViewerPan(panOffset + event.calculatePan(), viewportSize, nextScale)
                            }
                            scale = nextScale
                            zoomChanged(scale > 1.01f)
                            event.changes.forEach { it.consume() }
                        }
                    } while (event.changes.any { it.pressed })
                }
            }
            .graphicsLayer(
                scaleX = scale,
                scaleY = scale,
                translationX = panOffset.x,
                translationY = panOffset.y,
            ),
    )
}

@androidx.annotation.OptIn(UnstableApi::class)
@Composable
private fun VideoViewer(
    media: Media,
    repository: MomentoRepository,
    active: Boolean,
    toggleChrome: () -> Unit,
    playerChanged: (Long, ExoPlayer?) -> Unit,
    modifier: Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    if (!active) {
        MediaThumbnail(
            media = media,
            repository = repository,
            trashed = false,
            modifier = modifier,
        )
        return
    }
    val url by produceState<String?>(null, media.id) { value = repository.originalUrl(media.id) }
    val currentUrl = url
    if (currentUrl == null) {
        Box(modifier, contentAlignment = Alignment.Center) {
            CircularProgressIndicator(color = MaterialTheme.colorScheme.onBackground)
        }
        return
    }
    val player = remember(currentUrl) {
        val dataSourceFactory = OkHttpDataSource.Factory(repository.authenticatedHttpClient())
        ExoPlayer.Builder(context)
            .setMediaSourceFactory(DefaultMediaSourceFactory(context).setDataSourceFactory(dataSourceFactory))
            .build()
            .apply {
                setMediaItem(PlayerMediaItem.fromUri(currentUrl))
                prepare()
            }
    }
    var playbackError by remember(player) { mutableStateOf<String?>(null) }
    DisposableEffect(player) {
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                playbackError = "This video could not be played."
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }
    DisposableEffect(player, lifecycleOwner) {
        var resumeOnStart = false
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> {
                    resumeOnStart = player.playWhenReady
                    player.pause()
                }
                Lifecycle.Event.ON_START -> {
                    if (resumeOnStart) player.play()
                    resumeOnStart = false
                }
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            player.release()
        }
    }
    DisposableEffect(player, active) {
        if (active) {
            playerChanged(media.id, player)
        } else {
            player.pause()
        }
        onDispose {
            if (active) {
                player.pause()
                playerChanged(media.id, null)
            }
        }
    }
    Box(modifier) {
        AndroidView(
            factory = { playerContext ->
                PlayerView(playerContext).apply {
                    this.player = player
                    useController = false
                    setShowBuffering(PlayerView.SHOW_BUFFERING_WHEN_PLAYING)
                }
            },
            update = { it.player = player },
            modifier = Modifier.fillMaxSize(),
        )
        Box(Modifier.fillMaxSize().viewerChromeToggle(toggleChrome))
        playbackError?.let { message ->
            Surface(
                modifier = Modifier.align(Alignment.Center).padding(24.dp),
                color = Color(0xE6242824),
                contentColor = Color.White,
                shape = RoundedCornerShape(18.dp),
            ) {
                Column(
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 18.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(message, style = MaterialTheme.typography.titleMedium)
                    TextButton(onClick = {
                        playbackError = null
                        player.prepare()
                        player.play()
                    }) { Text("Try again", color = Color.White) }
                }
            }
        }
    }
}

@Composable
private fun ViewerTopControls(timestamp: ViewerTimestamp, close: () -> Unit, modifier: Modifier) {
    val floatingColors = momentoFloatingControlColors(darkTheme = true)
    Box(modifier.windowInsetsPadding(WindowInsets.safeDrawing).padding(12.dp)) {
        Surface(
            modifier = Modifier.align(Alignment.TopCenter),
            color = floatingColors.container,
            contentColor = floatingColors.content,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier.padding(horizontal = 18.dp, vertical = 8.dp),
            ) {
                Text(timestamp.date, style = MaterialTheme.typography.titleMedium)
                if (timestamp.time.isNotEmpty()) {
                    Text(timestamp.time, style = MaterialTheme.typography.labelSmall)
                }
            }
        }
        MomentoFloatingButton(
            modifier = Modifier.align(Alignment.TopEnd),
            onClick = close,
        ) { Icon(Icons.Default.Close, "Close media viewer") }
    }
}

@Composable
private fun ViewerBottomControls(
    media: List<Media>,
    currentIndex: Int,
    repository: MomentoRepository,
    player: ExoPlayer?,
    navigate: (Int) -> Unit,
    sharing: Boolean,
    share: () -> Unit,
    albums: () -> Unit,
    information: () -> Unit,
    trash: () -> Unit,
    userInteracted: () -> Unit,
    interactionChanged: (Boolean) -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors(darkTheme = true)
    Column(
        modifier = modifier.windowInsetsPadding(WindowInsets.safeDrawing).padding(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (player != null) {
            VideoPlaybackControls(
                player = player,
                userInteracted = userInteracted,
                interactionChanged = interactionChanged,
                modifier = Modifier.widthIn(max = 640.dp).fillMaxWidth(),
            )
        }
        ViewerFilmstrip(
            media = media,
            currentIndex = currentIndex,
            repository = repository,
            navigate = navigate,
            interactionChanged = interactionChanged,
            modifier = Modifier.widthIn(max = 640.dp).fillMaxWidth(),
        )
        Box(Modifier.fillMaxWidth().height(56.dp)) {
            MomentoFloatingButton(
                modifier = Modifier.align(Alignment.CenterStart),
                onClick = { userInteracted(); share() },
            ) {
                if (sharing) {
                    CircularProgressIndicator(
                        color = floatingColors.content,
                        modifier = Modifier.size(22.dp),
                    )
                } else {
                    Icon(Icons.Default.Share, "Share media")
                }
            }
            Surface(
                modifier = Modifier.align(Alignment.Center),
                color = floatingColors.container,
                contentColor = floatingColors.content,
                shape = MaterialTheme.shapes.extraLarge,
            ) {
                Row {
                    TextButton(
                        onClick = { userInteracted(); albums() },
                        colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                    ) {
                        Icon(Icons.Default.AddPhotoAlternate, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Albums")
                    }
                    TextButton(
                        onClick = { userInteracted(); information() },
                        colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                    ) {
                        Icon(Icons.Default.Info, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Info")
                    }
                }
            }
            MomentoFloatingButton(
                modifier = Modifier.align(Alignment.CenterEnd),
                onClick = { userInteracted(); trash() },
            ) { Icon(Icons.Default.Delete, "Move media to Trash") }
        }
    }
}

@Composable
private fun ViewerFilmstrip(
    media: List<Media>,
    currentIndex: Int,
    repository: MomentoRepository,
    navigate: (Int) -> Unit,
    interactionChanged: (Boolean) -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors(darkTheme = true)
    val listState = rememberLazyListState()
    val latestCurrentIndex by rememberUpdatedState(currentIndex)
    val latestNavigate by rememberUpdatedState(navigate)
    LaunchedEffect(currentIndex) {
        if (!listState.isScrollInProgress) listState.animateScrollToItem(currentIndex)
    }
    LaunchedEffect(listState) {
        androidx.compose.runtime.snapshotFlow { listState.isScrollInProgress }
            .distinctUntilChanged()
            .collect { scrolling ->
                interactionChanged(scrolling)
                if (scrolling) return@collect
                val layout = listState.layoutInfo
                val centeredIndex = centeredFilmstripIndex(
                    viewportStartOffset = layout.viewportStartOffset,
                    viewportEndOffset = layout.viewportEndOffset,
                    items = layout.visibleItemsInfo.map { item ->
                        FilmstripItemBounds(item.index, item.offset, item.size)
                    },
                )
                if (centeredIndex != null && centeredIndex != latestCurrentIndex) latestNavigate(centeredIndex)
            }
    }

    BoxWithConstraints(modifier.height(58.dp)) {
        val thumbnailSize = 46.dp
        val centeredPadding = ((maxWidth - thumbnailSize) / 2).coerceAtLeast(0.dp)
        val edgeVisibility = filmstripEdgeVisibility(
            canScrollBackward = listState.canScrollBackward,
            canScrollForward = listState.canScrollForward,
        )
        LazyRow(
            state = listState,
            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = centeredPadding),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxSize(),
        ) {
            itemsIndexed(media, key = { _, item -> item.id }) { index, item ->
                MediaThumbnail(
                    media = item,
                    repository = repository,
                    trashed = false,
                    modifier = Modifier
                        .size(thumbnailSize)
                        .clip(RoundedCornerShape(10.dp))
                        .border(
                            width = if (index == currentIndex) 2.dp else 0.dp,
                            color = floatingColors.content,
                            shape = RoundedCornerShape(10.dp),
                        )
                        .clickable { navigate(index) },
                )
            }
        }
        if (edgeVisibility.left) {
            Box(
                Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxHeight()
                    .width(36.dp)
                    .background(Brush.horizontalGradient(listOf(Color.Black, Color.Transparent))),
            )
        }
        if (edgeVisibility.right) {
            Box(
                Modifier
                    .align(Alignment.CenterEnd)
                    .fillMaxHeight()
                    .width(36.dp)
                    .background(Brush.horizontalGradient(listOf(Color.Transparent, Color.Black))),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun VideoPlaybackControls(
    player: ExoPlayer,
    userInteracted: () -> Unit,
    interactionChanged: (Boolean) -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors(darkTheme = true)
    var seekState by remember(player) { mutableStateOf(ViewerSeekState.initial()) }
    var playing by remember(player) { mutableStateOf(player.playWhenReady) }
    var muted by remember(player) { mutableStateOf(player.volume == 0f) }
    val seekRangeEnd = seekState.durationMs.coerceAtLeast(1L).toFloat()
    val displayedPosition = seekState.displayedPositionMs

    LaunchedEffect(player) {
        while (isActive) {
            seekState = seekState.synchronize(
                positionMs = player.currentPosition,
                durationMs = player.duration,
            )
            playing = player.playWhenReady
            muted = player.volume == 0f
            delay(200)
        }
    }

    Surface(
        modifier = modifier.padding(horizontal = 12.dp),
        color = Color.Transparent,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.small,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(horizontal = 2.dp),
        ) {
            IconButton(
                onClick = {
                    userInteracted()
                    if (player.playWhenReady) player.pause() else player.play()
                    playing = player.playWhenReady
                },
            ) {
                Icon(
                    imageVector = if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                    contentDescription = if (playing) "Pause video" else "Play video",
                )
            }
            Slider(
                value = displayedPosition,
                onValueChange = { value ->
                    userInteracted()
                    if (!seekState.dragging) interactionChanged(true)
                    seekState = seekState.dragTo(value)
                },
                onValueChangeFinished = {
                    val commit = seekState.commitDrag()
                    if (commit != null) {
                        seekState = commit.first
                        player.seekTo(commit.second)
                    }
                    interactionChanged(false)
                },
                valueRange = 0f..seekRangeEnd,
                enabled = seekState.durationMs > 0,
                modifier = Modifier.weight(1f).height(48.dp),
                thumb = {
                    Box(
                        Modifier
                            .size(if (seekState.dragging) 12.dp else 9.dp)
                            .background(floatingColors.content, CircleShape),
                    )
                },
                track = { sliderState ->
                    val progress = playbackProgressFraction(sliderState.value, seekState.durationMs)
                    Canvas(Modifier.fillMaxWidth().height(3.dp)) {
                        val centerY = size.height / 2f
                        drawLine(
                            color = floatingColors.content.copy(alpha = 0.32f),
                            start = Offset(0f, centerY),
                            end = Offset(size.width, centerY),
                            strokeWidth = size.height,
                            cap = StrokeCap.Round,
                        )
                        drawLine(
                            color = floatingColors.content,
                            start = Offset(0f, centerY),
                            end = Offset(size.width * progress, centerY),
                            strokeWidth = size.height,
                            cap = StrokeCap.Round,
                        )
                    }
                },
            )
            Text(
                "${formatPlaybackTime(displayedPosition.toLong())} / ${formatPlaybackTime(seekState.durationMs)}",
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
            )
            IconButton(
                onClick = {
                    userInteracted()
                    player.volume = if (player.volume == 0f) 1f else 0f
                    muted = player.volume == 0f
                },
            ) {
                Icon(
                    imageVector = if (muted) Icons.AutoMirrored.Filled.VolumeOff else Icons.AutoMirrored.Filled.VolumeUp,
                    contentDescription = if (muted) "Unmute video" else "Mute video",
                )
            }
        }
    }
}

private fun Modifier.viewerChromeToggle(toggle: () -> Unit): Modifier = pointerInput(toggle) {
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        var maxPointerCount = 1
        var movedBeyondTouchSlop = false
        var event: PointerEvent
        do {
            event = awaitPointerEvent(PointerEventPass.Initial)
            maxPointerCount = maxOf(maxPointerCount, event.changes.count { it.pressed })
            movedBeyondTouchSlop = movedBeyondTouchSlop || event.changes.any { change ->
                (change.position - down.position).getDistance() > viewConfiguration.touchSlop
            }
        } while (event.changes.any { it.pressed })
        if (shouldToggleViewerChrome(maxPointerCount, movedBeyondTouchSlop)) toggle()
    }
}

@Composable
private fun MediaInformationContent(media: Media, modifier: Modifier) {
    val rows = remember(media) { mediaMetadataRows(media) }
    LazyColumn(modifier) {
        item {
            Text(
                "Information",
                style = MaterialTheme.typography.headlineSmall,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
            )
        }
        items(rows) { (label, value) ->
            ListItem(
                headlineContent = { Text(value) },
                supportingContent = { Text(label) },
            )
            HorizontalDivider()
        }
        item { Spacer(Modifier.height(24.dp)) }
    }
}

private fun parseMediaDateTime(value: String): LocalDateTime? {
    try {
        return OffsetDateTime.parse(value).atZoneSameInstant(ZoneId.systemDefault()).toLocalDateTime()
    } catch (_: DateTimeException) {
    }
    try {
        return Instant.parse(value).atZone(ZoneId.systemDefault()).toLocalDateTime()
    } catch (_: DateTimeException) {
    }
    try {
        return LocalDateTime.parse(value, DateTimeFormatter.ISO_LOCAL_DATE_TIME)
    } catch (_: DateTimeException) {
        return null
    }
}

private fun formatFileSize(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val kibibytes = bytes / 1024.0
    if (kibibytes < 1024) return String.format(Locale.ENGLISH, "%.1f KiB", kibibytes)
    return String.format(Locale.ENGLISH, "%.1f MiB", kibibytes / 1024.0)
}

private fun formatDuration(seconds: Double): String {
    val totalSeconds = seconds.toLong().coerceAtLeast(0)
    return "%d:%02d".format(Locale.ENGLISH, totalSeconds / 60, totalSeconds % 60)
}

fun formatPlaybackTime(milliseconds: Long): String {
    val totalSeconds = milliseconds.coerceAtLeast(0L) / 1_000L
    val hours = totalSeconds / 3_600L
    val minutes = totalSeconds % 3_600L / 60L
    val seconds = totalSeconds % 60L
    if (hours > 0L) return "%d:%02d:%02d".format(Locale.ENGLISH, hours, minutes, seconds)
    return "%d:%02d".format(Locale.ENGLISH, minutes, seconds)
}

private const val SHARE_CACHE_DIRECTORY = "shared_media"
private const val SHARE_CACHE_EXPIRY_MILLIS = 24L * 60 * 60 * 1_000
