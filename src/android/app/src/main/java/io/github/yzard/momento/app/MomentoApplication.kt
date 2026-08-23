package io.github.yzard.momento.app

import android.view.autofill.AutofillManager
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.AddCircle
import androidx.compose.material.icons.filled.CollectionsBookmark
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.AutofillNode
import androidx.compose.ui.autofill.AutofillType
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalAutofill
import androidx.compose.ui.platform.LocalAutofillTree
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.navigation.Destination
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.data.normalizeServerOrigin
import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.User
import io.github.yzard.momento.feature.admin.AdminScreen
import io.github.yzard.momento.feature.albums.AlbumsScreen
import io.github.yzard.momento.feature.deduplicate.DeduplicateScreen
import io.github.yzard.momento.feature.faces.FacesScreen
import io.github.yzard.momento.feature.map.NativeMapScreen
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.places.PlacesScreen
import io.github.yzard.momento.feature.search.SearchScreen
import io.github.yzard.momento.feature.settings.SettingsScreen
import io.github.yzard.momento.feature.timeline.TimelineScreen
import io.github.yzard.momento.feature.trash.TrashScreen
import io.github.yzard.momento.feature.viewer.ViewerScreen
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException

@Composable
fun MomentoApplication(
    settingsStore: SettingsStore,
    repository: MomentoRepository,
    tokenStore: EncryptedTokenStore,
) {
    val settingsFlow: Flow<Settings?> = remember(settingsStore) { settingsStore.settings.map { it } }
    val settings by settingsFlow.collectAsState(initial = null)
    val authenticated by tokenStore.isAuthenticated.collectAsState()
    var user by remember { mutableStateOf<User?>(null) }
    val loadedSettings = settings

    if (loadedSettings == null) {
        MomentoTheme(ThemePreference.SYSTEM) { LoadingState() }
        return
    }

    MomentoTheme(loadedSettings.themePreference) {
        when {
            loadedSettings.origin == null -> ServerScreen(settingsStore, repository)
            !authenticated -> LoginScreen(repository) { user = it }
            else -> PhotoShell(repository, settingsStore, user) { user = it }
        }
    }
}

@Composable
private fun ServerScreen(settingsStore: SettingsStore, repository: MomentoRepository) {
    var origin by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var insecureWarning by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Surface(Modifier.fillMaxSize().padding(28.dp)) {
        Column(verticalArrangement = Arrangement.Center) {
            Text("Your Momento", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
            Text("Connect to the server that keeps your library private.", Modifier.padding(vertical = 12.dp))
            OutlinedTextField(
                value = origin,
                onValueChange = { origin = it },
                label = { Text("Server origin") },
                placeholder = { Text("https://photos.example.com") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp)) }
            Button(
                onClick = {
                    if (origin.startsWith("http://")) insecureWarning = true
                    else scope.launch { connect(origin, settingsStore, repository) { error = it } }
                },
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
            ) { Text("Connect") }
        }
    }
    if (insecureWarning) {
        AlertDialog(
            onDismissRequest = { insecureWarning = false },
            title = { Text("Unencrypted server") },
            text = { Text("HTTP can expose your library and account. Continue only on a network you trust.") },
            confirmButton = {
                TextButton({
                    insecureWarning = false
                    scope.launch { connect(origin, settingsStore, repository) { error = it } }
                }) { Text("Use HTTP") }
            },
            dismissButton = { TextButton({ insecureWarning = false }) { Text("Cancel") } },
        )
    }
}

private suspend fun connect(
    origin: String,
    settingsStore: SettingsStore,
    repository: MomentoRepository,
    onError: (String) -> Unit,
) {
    try {
        val normalized = normalizeServerOrigin(origin)
        repository.capabilities(normalized)
        settingsStore.setOrigin(normalized)
    } catch (exception: IllegalArgumentException) {
        onError(exception.message ?: "Invalid server")
    } catch (_: IOException) {
        onError("Could not reach server")
    } catch (_: HttpException) {
        onError("Server did not accept the connection")
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Suppress("DEPRECATION")
@Composable
private fun LoginScreen(repository: MomentoRepository, signedIn: (User) -> Unit) {
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var signingIn by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val passwordFocusRequester = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val context = LocalContext.current
    val autofill = LocalAutofill.current
    val autofillTree = LocalAutofillTree.current
    val usernameAutofillNode = remember {
        AutofillNode(
            autofillTypes = listOf(AutofillType.Username),
            onFill = { username = it },
        )
    }
    val passwordAutofillNode = remember {
        AutofillNode(
            autofillTypes = listOf(AutofillType.Password),
            onFill = { password = it },
        )
    }
    DisposableEffect(autofillTree, usernameAutofillNode, passwordAutofillNode) {
        autofillTree += usernameAutofillNode
        autofillTree += passwordAutofillNode
        onDispose {
            autofillTree.children.remove(usernameAutofillNode.id)
            autofillTree.children.remove(passwordAutofillNode.id)
        }
    }

    fun submit() {
        if (signingIn) return
        focusManager.clearFocus()
        keyboardController?.hide()
        signingIn = true
        scope.launch {
            try {
                val user = repository.login(username, password)
                context.getSystemService(AutofillManager::class.java)?.commit()
                signedIn(user)
                repository.completeLogin()
            } catch (_: HttpException) {
                error = "Incorrect username or password"
            } catch (_: IOException) {
                error = "Could not reach the server"
            } catch (_: kotlinx.serialization.SerializationException) {
                error = "Server returned an invalid response"
            } finally {
                signingIn = false
            }
        }
    }

    Surface(Modifier.fillMaxSize().imePadding().padding(28.dp)) {
        Column(verticalArrangement = Arrangement.Center) {
            Text("Sign in", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
            OutlinedTextField(
                value = username,
                onValueChange = { username = it },
                label = { Text("Username") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                keyboardActions = KeyboardActions(onNext = { passwordFocusRequester.requestFocus() }),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 20.dp)
                    .autofill(usernameAutofillNode, autofill),
            )
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                label = { Text("Password") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }),
                visualTransformation = PasswordVisualTransformation(),
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(passwordFocusRequester)
                    .autofill(passwordAutofillNode, autofill),
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                onClick = { submit() },
                enabled = !signingIn,
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
            ) { Text("Sign in") }
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Suppress("DEPRECATION")
private fun Modifier.autofill(
    autofillNode: AutofillNode,
    autofill: androidx.compose.ui.autofill.Autofill?,
): Modifier = this
    .onGloballyPositioned { coordinates ->
        autofillNode.boundingBox = coordinates.boundsInWindow().takeUnless { it == Rect.Zero }
    }
    .onFocusChanged { focusState ->
        if (focusState.isFocused) {
            autofill?.requestAutofillForNode(autofillNode)
        } else {
            autofill?.cancelAutofillForNode(autofillNode)
        }
    }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PhotoShell(
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    initialUser: User?,
    userLoaded: (User) -> Unit,
) {
    var destination by remember { mutableStateOf(Destination.PHOTOS) }
    var viewerMedia by remember { mutableStateOf<List<Media>?>(null) }
    var viewerIndex by remember { mutableStateOf(0) }
    var user by remember { mutableStateOf(initialUser) }
    val scope = rememberCoroutineScope()

    BackHandler(enabled = destination != Destination.PHOTOS) { destination = Destination.PHOTOS }
    LaunchedEffect(Unit) {
        if (user == null) {
            try {
                repository.currentUser().also { loadedUser ->
                    user = loadedUser
                    userLoaded(loadedUser)
                }
            } catch (_: IOException) {
            } catch (_: HttpException) {
            }
        }
    }
    viewerMedia?.let { media ->
        ViewerScreen(media, viewerIndex, repository) { viewerMedia = null }
        return
    }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val useNavigationRail = maxWidth >= 600.dp
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(destination.label, fontWeight = FontWeight.Bold) },
                    actions = {
                        IconButton({ destination = Destination.SETTINGS }) {
                            Icon(Icons.Default.AccountCircle, "Profile and settings")
                        }
                    },
                )
            },
            floatingActionButton = {
                FloatingActionButton(
                    onClick = { destination = Destination.SEARCH },
                    modifier = Modifier.padding(bottom = if (useNavigationRail) 0.dp else 72.dp),
                ) {
                    Icon(Icons.Default.Search, "Search")
                }
            },
        ) { padding ->
            Row(Modifier.fillMaxSize().padding(padding)) {
                if (useNavigationRail) {
                    ShellNavigationRail(destination) { destination = it }
                }
                Box(Modifier.weight(1f).fillMaxSize()) {
                    Box(Modifier.fillMaxSize().padding(bottom = if (useNavigationRail) 0.dp else 76.dp)) {
                        ShellDestination(
                            destination = destination,
                            repository = repository,
                            settingsStore = settingsStore,
                            user = user,
                            openDestination = { destination = it },
                            openMedia = { media, index ->
                                viewerMedia = media
                                viewerIndex = index
                            },
                            logout = { scope.launch { repository.logout() } },
                        )
                    }
                    if (!useNavigationRail) {
                        PhoneNavigationPill(destination, { destination = it }, Modifier.align(Alignment.BottomCenter))
                    }
                }
            }
        }
    }
}

@Composable
private fun ShellNavigationRail(destination: Destination, select: (Destination) -> Unit) {
    NavigationRail {
        shellDestinations.forEach { item ->
            NavigationRailItem(
                selected = destination == item,
                onClick = { select(item) },
                icon = { Icon(shellIcon(item), item.label) },
                label = { Text(item.label) },
            )
        }
    }
}

@Composable
private fun PhoneNavigationPill(destination: Destination, select: (Destination) -> Unit, modifier: Modifier) {
    Surface(
        modifier = modifier.padding(bottom = 20.dp),
        shape = MaterialTheme.shapes.extraLarge,
        shadowElevation = 8.dp,
        tonalElevation = 3.dp,
    ) {
        Row(Modifier.padding(horizontal = 8.dp, vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            shellDestinations.forEach { item ->
                val selected = destination == item
                Surface(
                    shape = MaterialTheme.shapes.large,
                    color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surface,
                    modifier = Modifier.clip(MaterialTheme.shapes.large).clickable { select(item) },
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.padding(horizontal = 14.dp, vertical = 7.dp)) {
                        Icon(shellIcon(item), item.label, modifier = Modifier.size(20.dp))
                        Text(item.label, style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
        }
    }
}

private val shellDestinations = listOf(Destination.PHOTOS, Destination.COLLECTIONS, Destination.CREATE)

private fun shellIcon(destination: Destination) = when (destination) {
    Destination.PHOTOS -> Icons.Default.PhotoLibrary
    Destination.COLLECTIONS -> Icons.Default.CollectionsBookmark
    Destination.CREATE -> Icons.Default.AddCircle
    else -> Icons.Default.Folder
}

@Composable
private fun ShellDestination(
    destination: Destination,
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    user: User?,
    openDestination: (Destination) -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
    logout: () -> Unit,
) {
    when (destination) {
        Destination.PHOTOS -> TimelineScreen(repository, openMedia)
        Destination.COLLECTIONS -> CollectionsScreen(openDestination)
        Destination.CREATE -> CreateAlbumScreen(repository) { openDestination(Destination.ALBUMS) }
        Destination.SEARCH -> SearchScreen(repository, openMedia)
        Destination.SETTINGS -> SettingsScreen(repository, settingsStore, user, { openDestination(Destination.ADMIN) }, logout)
        Destination.ALBUMS -> AlbumsScreen(repository, openMedia)
        Destination.MAP -> NativeMapScreen(repository) { media -> openMedia(media, 0) }
        Destination.PLACES -> PlacesScreen(repository, openMedia)
        Destination.FACES -> FacesScreen(repository, user?.role == "admin", openMedia)
        Destination.DEDUPLICATE -> DeduplicateScreen(repository, user?.role == "admin")
        Destination.TRASH -> TrashScreen(repository)
        Destination.ADMIN -> AdminScreen(repository)
        Destination.VIEWER -> Unit
    }
}

@Composable
private fun CollectionsScreen(open: (Destination) -> Unit) {
    val collections = listOf(
        Destination.ALBUMS,
        Destination.MAP,
        Destination.PLACES,
        Destination.FACES,
        Destination.DEDUPLICATE,
        Destination.TRASH,
    )
    LazyColumn(Modifier.fillMaxSize().padding(16.dp), contentPadding = PaddingValues(bottom = 100.dp)) {
        items(collections) { collection ->
            ListItem(
                headlineContent = { Text(collection.label) },
                supportingContent = { Text(if (collection == Destination.DEDUPLICATE) "Find similar photos" else "Browse your library") },
                leadingContent = { Icon(Icons.Default.Folder, null) },
                modifier = Modifier.clickable { open(collection) },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun CreateAlbumScreen(repository: MomentoRepository, complete: () -> Unit) {
    var name by remember { mutableStateOf("") }
    var description by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var results by remember { mutableStateOf<List<Media>>(emptyList()) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(query) {
        if (query.isBlank()) {
            results = emptyList()
            return@LaunchedEffect
        }
        delay(300)
        try {
            results = repository.search(query)
            error = null
        } catch (_: IOException) {
            error = "Could not search the library"
        } catch (_: HttpException) {
            error = "Could not search the library"
        }
    }
    LazyColumn(Modifier.fillMaxSize().padding(24.dp), contentPadding = PaddingValues(bottom = 100.dp)) {
        item {
            Text("New album", style = MaterialTheme.typography.headlineMedium)
            OutlinedTextField(name, { name = it }, label = { Text("Album name") }, modifier = Modifier.fillMaxWidth().padding(top = 16.dp))
            OutlinedTextField(description, { description = it }, label = { Text("Description") }, modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
            OutlinedTextField(query, { query = it }, label = { Text("Optionally add photos") }, modifier = Modifier.fillMaxWidth().padding(top = 16.dp))
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
        items(results, key = { it.id }) { media ->
            ListItem(
                headlineContent = { Text(media.originalFilename) },
                supportingContent = { Text(if (media.id in selectedIds) "Selected" else "Tap to select") },
                modifier = Modifier.clickable {
                    selectedIds = if (media.id in selectedIds) selectedIds - media.id else selectedIds + media.id
                },
            )
        }
        item {
            Button(
                onClick = {
                    if (name.isBlank()) {
                        error = "Album name is required"
                        return@Button
                    }
                    scope.launch {
                        try {
                            val album = repository.createAlbum(name, description.ifBlank { null })
                            if (selectedIds.isNotEmpty()) repository.addAlbumMedia(album.id, selectedIds.toList())
                            complete()
                        } catch (_: IOException) {
                            error = "Could not create album"
                        } catch (_: HttpException) {
                            error = "Could not create album"
                        }
                    }
                },
                modifier = Modifier.padding(top = 12.dp),
            ) { Text("Create album") }
        }
    }
}
