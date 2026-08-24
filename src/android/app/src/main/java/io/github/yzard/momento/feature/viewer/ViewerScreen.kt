package io.github.yzard.momento.feature.viewer

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
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
import androidx.compose.material3.SheetValue
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.MediaItem as PlayerMediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.PlayerView
import coil.compose.AsyncImage
import coil.request.ImageRequest
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.model.Album
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.feature.media.MediaThumbnail
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
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

data class ViewerTimestamp(val date: String, val time: String)
data class FilmstripItemBounds(val index: Int, val offset: Int, val size: Int)

private enum class ViewerSheet { ALBUMS, INFORMATION }

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
    media.locationCity?.let { city ->
        "Location" to listOfNotNull(city, media.locationState, media.locationCountry).joinToString(", ")
    },
    media.gpsLatitude?.let { latitude ->
        media.gpsLongitude?.let { longitude -> "Coordinates" to "$latitude, $longitude" }
    },
    "Created" to media.createdAt,
)

fun shareCacheFilename(media: Media): String {
    val safeName = media.originalFilename.replace(Regex("[^A-Za-z0-9._-]"), "_").ifBlank { media.filename }
    return "${media.id}-$safeName"
}

fun shouldToggleViewerChrome(maxPointerCount: Int, movedBeyondTouchSlop: Boolean): Boolean =
    maxPointerCount == 1 && !movedBeyondTouchSlop

fun boundedPlaybackPosition(positionMs: Float, durationMs: Long): Long {
    if (durationMs <= 0) return 0
    return positionMs.toLong().coerceIn(0, durationMs)
}

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
    mediaChanged: () -> Unit,
    close: () -> Unit,
) {
    var items by remember(media) { mutableStateOf(media) }
    val pagerState = rememberPagerState(
        initialPage = viewerIndex(initialIndex, 0, media.size),
        pageCount = { items.size },
    )
    var activeSheet by remember { mutableStateOf<ViewerSheet?>(null) }
    var confirmTrash by remember { mutableStateOf(false) }
    var trashing by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var chromeVisible by remember { mutableStateOf(true) }
    var activeVideoPlayer by remember { mutableStateOf<Pair<Long, ExoPlayer>?>(null) }
    val context = LocalContext.current
    val screenHeight = LocalConfiguration.current.screenHeightDp.dp
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    val mediaLift by animateDpAsState(
        targetValue = when {
            activeSheet == null -> 0.dp
            sheetState.targetValue == SheetValue.Expanded -> screenHeight * 0.55f
            else -> screenHeight * 0.35f
        },
        label = "viewer media lift",
    )

    if (items.isEmpty()) {
        LaunchedEffect(Unit) { close() }
        return
    }

    val index = pagerState.currentPage.coerceIn(0, items.lastIndex)
    val item = items[index]
    val timestamp = remember(item.id, item.dateTaken, item.createdAt) { viewerTimestamp(item) }

    suspend fun shareCurrent() {
        if (sharing) return
        sharing = true
        try {
            val sharedFile = File(File(context.cacheDir, "shared_media"), shareCacheFilename(item))
            repository.downloadOriginal(item.id, sharedFile)
            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                sharedFile,
            )
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = item.mimeType ?: if (item.mediaType == "video") "video/*" else "image/*"
                putExtra(Intent.EXTRA_STREAM, uri)
                putExtra(Intent.EXTRA_TITLE, item.originalFilename)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            context.startActivity(Intent.createChooser(intent, "Share media"))
        } catch (_: IOException) {
            error = "Could not prepare this media for sharing"
        } catch (_: ActivityNotFoundException) {
            error = "No application is available to share this media"
        } finally {
            sharing = false
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
            mediaChanged()
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

    BackHandler(onBack = close)

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        HorizontalPager(
            state = pagerState,
            key = { page -> items[page].id },
            userScrollEnabled = activeSheet == null,
            modifier = Modifier.fillMaxSize().padding(bottom = mediaLift),
        ) { page ->
            ViewerMedia(
                media = items[page],
                repository = repository,
                active = page == index,
                toggleChrome = { chromeVisible = !chromeVisible },
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
            visible = chromeVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.fillMaxWidth().align(Alignment.TopCenter),
        ) {
            ViewerTopControls(timestamp = timestamp, close = close, modifier = Modifier.fillMaxWidth())
        }
        AnimatedVisibility(
            visible = chromeVisible,
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
                albums = { activeSheet = ViewerSheet.ALBUMS },
                information = { activeSheet = ViewerSheet.INFORMATION },
                trash = { confirmTrash = true },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    if (activeSheet != null) {
        ModalBottomSheet(
            onDismissRequest = { activeSheet = null },
            sheetState = sheetState,
            containerColor = MaterialTheme.colorScheme.background,
            contentColor = MaterialTheme.colorScheme.onBackground,
        ) {
            when (activeSheet) {
                ViewerSheet.ALBUMS -> ViewerAlbumsSheet(
                    media = item,
                    repository = repository,
                    close = { activeSheet = null },
                )
                ViewerSheet.INFORMATION -> MediaInformationSheet(item)
                null -> Unit
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
    playerChanged: (Long, ExoPlayer?) -> Unit,
    modifier: Modifier,
) {
    if (media.mediaType == "video") {
        VideoViewer(media, repository, active, toggleChrome, playerChanged, modifier)
        return
    }

    val context = LocalContext.current
    val url by produceState<String?>(null, media.id) { value = repository.previewUrl(media.id) }
    var scale by remember(media.id) { mutableFloatStateOf(1f) }
    AsyncImage(
        model = url?.let { ImageRequest.Builder(context).data(it).build() },
        imageLoader = repository.authenticatedImageLoader(context),
        contentDescription = media.originalFilename,
        contentScale = ContentScale.Fit,
        modifier = modifier
            .viewerChromeToggle(toggleChrome)
            .pointerInput(media.id) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false)
                    var event: PointerEvent
                    do {
                        event = awaitPointerEvent()
                        if (event.changes.count { it.pressed } >= 2) {
                            scale = (scale * event.calculateZoom()).coerceIn(1f, 5f)
                            event.changes.forEach { it.consume() }
                        }
                    } while (event.changes.any { it.pressed })
                }
            }
            .graphicsLayer(scaleX = scale, scaleY = scale),
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
    }
}

@Composable
private fun ViewerTopControls(timestamp: ViewerTimestamp, close: () -> Unit, modifier: Modifier) {
    val floatingColors = momentoFloatingControlColors()
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
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    Column(
        modifier = modifier.windowInsetsPadding(WindowInsets.safeDrawing).padding(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (player != null) {
            VideoPlaybackControls(player, Modifier.widthIn(max = 560.dp).fillMaxWidth())
        }
        ViewerFilmstrip(
            media = media,
            currentIndex = currentIndex,
            repository = repository,
            navigate = navigate,
            modifier = Modifier.widthIn(max = 560.dp).fillMaxWidth(),
        )
        Box(Modifier.fillMaxWidth().height(56.dp)) {
            MomentoFloatingButton(
                modifier = Modifier.align(Alignment.CenterStart),
                onClick = share,
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
                        onClick = albums,
                        colors = ButtonDefaults.textButtonColors(contentColor = floatingColors.content),
                    ) {
                        Icon(Icons.Default.AddPhotoAlternate, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Albums")
                    }
                    TextButton(
                        onClick = information,
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
                onClick = trash,
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
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    val listState = rememberLazyListState()
    val latestCurrentIndex by rememberUpdatedState(currentIndex)
    val latestNavigate by rememberUpdatedState(navigate)
    LaunchedEffect(currentIndex) { listState.animateScrollToItem(currentIndex) }
    LaunchedEffect(listState) {
        androidx.compose.runtime.snapshotFlow { listState.isScrollInProgress }
            .distinctUntilChanged()
            .filter { scrolling -> !scrolling }
            .collect {
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

    Surface(
        modifier = modifier.padding(horizontal = 68.dp),
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        BoxWithConstraints(Modifier.fillMaxWidth().height(58.dp)) {
            val thumbnailSize = 46.dp
            val centeredPadding = ((maxWidth - thumbnailSize) / 2).coerceAtLeast(0.dp)
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
        }
    }
}

@Composable
private fun VideoPlaybackControls(player: ExoPlayer, modifier: Modifier) {
    val floatingColors = momentoFloatingControlColors()
    var positionMs by remember(player) { mutableLongStateOf(0L) }
    var durationMs by remember(player) { mutableLongStateOf(0L) }
    var previewPositionMs by remember(player) { mutableFloatStateOf(0f) }
    var dragging by remember(player) { mutableStateOf(false) }
    var playing by remember(player) { mutableStateOf(player.playWhenReady) }
    var muted by remember(player) { mutableStateOf(player.volume == 0f) }
    val seekRangeEnd = durationMs.coerceAtLeast(1L).toFloat()
    val displayedPosition = (if (dragging) previewPositionMs else positionMs.toFloat())
        .coerceIn(0f, seekRangeEnd)

    LaunchedEffect(player) {
        while (isActive) {
            if (!dragging) positionMs = player.currentPosition.coerceAtLeast(0L)
            durationMs = player.duration.coerceAtLeast(0L)
            playing = player.playWhenReady
            muted = player.volume == 0f
            delay(200)
        }
    }

    Surface(
        modifier = modifier.padding(horizontal = 36.dp),
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(horizontal = 6.dp),
        ) {
            IconButton(
                onClick = {
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
                    dragging = true
                    previewPositionMs = value
                },
                onValueChangeFinished = {
                    val target = boundedPlaybackPosition(previewPositionMs, durationMs)
                    player.seekTo(target)
                    positionMs = target
                    dragging = false
                },
                valueRange = 0f..seekRangeEnd,
                enabled = durationMs > 0,
                modifier = Modifier.weight(1f),
            )
            IconButton(
                onClick = {
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
private fun ViewerAlbumsSheet(media: Media, repository: MomentoRepository, close: () -> Unit) {
    var albums by remember(media.id) { mutableStateOf<List<Album>?>(null) }
    var error by remember(media.id) { mutableStateOf<String?>(null) }
    var addingAlbumId by remember(media.id) { mutableStateOf<Long?>(null) }
    val scope = rememberCoroutineScope()

    fun addToAlbum(album: Album) {
        scope.launch {
            addingAlbumId = album.id
            try {
                repository.addAlbumMedia(album.id, listOf(media.id))
                close()
            } catch (_: IOException) {
                error = "Could not add this media to ${album.name}"
            } catch (_: HttpException) {
                error = "Could not add this media to ${album.name}"
            } catch (_: SerializationException) {
                error = "Could not add this media to ${album.name}"
            } finally {
                addingAlbumId = null
            }
        }
    }

    LaunchedEffect(media.id) {
        try {
            albums = repository.albums()
        } catch (_: IOException) {
            error = "Could not load albums"
        } catch (_: HttpException) {
            error = "Could not load albums"
        } catch (_: SerializationException) {
            error = "Could not load albums"
        }
    }

    Column(Modifier.fillMaxWidth().fillMaxHeight(0.72f)) {
        Text(
            "Add to album",
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
        )
        when {
            error != null -> Text(error!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(20.dp))
            albums == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            albums!!.isEmpty() -> Text("No albums yet", modifier = Modifier.padding(20.dp))
            else -> LazyColumn {
                items(albums!!, key = { it.id }) { album ->
                    ListItem(
                        headlineContent = { Text(album.name) },
                        supportingContent = { Text("${album.mediaCount} items") },
                        trailingContent = {
                            if (addingAlbumId == album.id) {
                                CircularProgressIndicator(modifier = Modifier.width(20.dp))
                            } else {
                                Text("Add")
                            }
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 8.dp)
                            .clickable(enabled = addingAlbumId == null) { addToAlbum(album) },
                    )
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun MediaInformationSheet(media: Media) {
    val rows = remember(media) { mediaMetadataRows(media) }
    LazyColumn(Modifier.fillMaxWidth().fillMaxHeight(0.72f)) {
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
