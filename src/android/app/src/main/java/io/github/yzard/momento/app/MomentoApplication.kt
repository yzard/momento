package io.github.yzard.momento.app

import android.view.autofill.AutofillManager
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Face
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.filled.Place
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Screenshot
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDrawerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.AutofillNode
import androidx.compose.ui.autofill.AutofillType
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalAutofill
import androidx.compose.ui.platform.LocalAutofillTree
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.Role
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.navigation.Destination
import io.github.yzard.momento.app.navigation.isTimelinePage
import io.github.yzard.momento.app.navigation.hasFloatingTitle
import io.github.yzard.momento.app.navigation.timelineSubpageDestinations
import io.github.yzard.momento.app.navigation.utilityDrawerDestinations
import io.github.yzard.momento.app.navigation.webDrawerDestinations
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
import io.github.yzard.momento.feature.auth.LoginRequirement
import io.github.yzard.momento.feature.auth.PasswordChangeFields
import io.github.yzard.momento.feature.auth.loginRequirement
import io.github.yzard.momento.feature.auth.validateNewPassword
import io.github.yzard.momento.feature.backup.BackupMediaAccess
import io.github.yzard.momento.feature.backup.currentBackupMediaAccess
import io.github.yzard.momento.feature.backup.schedulePeriodicBackup
import io.github.yzard.momento.feature.deduplicate.DeduplicateScreen
import io.github.yzard.momento.feature.faces.FacesScreen
import io.github.yzard.momento.feature.map.NativeMapScreen
import io.github.yzard.momento.feature.media.LoadingState
import io.github.yzard.momento.feature.places.PlacesScreen
import io.github.yzard.momento.feature.settings.SettingsScreen
import io.github.yzard.momento.feature.timeline.TimelinePage
import io.github.yzard.momento.feature.timeline.TimelinePeriod
import io.github.yzard.momento.feature.timeline.TimelineScreen
import io.github.yzard.momento.feature.timeline.normalizedTimelineSearchQuery
import io.github.yzard.momento.feature.trash.TrashScreen
import io.github.yzard.momento.feature.viewer.ViewerScreen
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
    val context = LocalContext.current
    val settingsFlow: Flow<Settings?> = remember(settingsStore) { settingsStore.settings.map { it } }
    val settings by settingsFlow.collectAsState(initial = null)
    val authenticated by tokenStore.isAuthenticated.collectAsState()
    var user by remember { mutableStateOf<User?>(null) }
    var passwordChangeUser by remember { mutableStateOf<User?>(null) }
    val loadedSettings = settings

    if (loadedSettings == null) {
        MomentoTheme(ThemePreference.SYSTEM) { LoadingState() }
        return
    }

    LaunchedEffect(authenticated, loadedSettings.origin, loadedSettings.mobileDataEnabled) {
        if (
            authenticated &&
            loadedSettings.origin != null &&
            currentBackupMediaAccess(context) != BackupMediaAccess.DENIED
        ) {
            schedulePeriodicBackup(context.applicationContext, loadedSettings.mobileDataEnabled)
        }
    }

    MomentoTheme(loadedSettings.themePreference) {
        when {
            loadedSettings.origin == null -> ServerScreen(settingsStore, repository)
            passwordChangeUser != null -> ForcedPasswordChangeScreen(
                repository = repository,
                username = requireNotNull(passwordChangeUser).username,
                passwordChanged = {
                    passwordChangeUser = null
                    user = null
                },
            )
            !authenticated -> LoginScreen(
                repository = repository,
                signedIn = { loadedUser -> user = loadedUser },
                passwordChangeRequired = { loadedUser -> passwordChangeUser = loadedUser },
            )
            else -> MainShell(repository, settingsStore, user) { loadedUser ->
                if (loginRequirement(loadedUser) == LoginRequirement.CHANGE_PASSWORD) {
                    repository.requirePasswordChange()
                    user = null
                    passwordChangeUser = loadedUser
                } else {
                    user = loadedUser
                }
            }
        }
    }
}

@Composable
private fun ServerScreen(settingsStore: SettingsStore, repository: MomentoRepository) {
    var origin by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var insecureWarning by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onBackground,
    ) {
        Column(
            modifier = Modifier.fillMaxSize().padding(28.dp),
            verticalArrangement = Arrangement.Center,
        ) {
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
                    if (shouldConfirmCleartextOrigin(origin, BuildConfig.ALLOW_CLEARTEXT_TRAFFIC)) {
                        insecureWarning = true
                    } else {
                        scope.launch {
                            connect(
                                origin = origin,
                                allowCleartextTraffic = BuildConfig.ALLOW_CLEARTEXT_TRAFFIC,
                                settingsStore = settingsStore,
                                repository = repository,
                                onError = { error = it },
                            )
                        }
                    }
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
                    scope.launch {
                        connect(
                            origin = origin,
                            allowCleartextTraffic = BuildConfig.ALLOW_CLEARTEXT_TRAFFIC,
                            settingsStore = settingsStore,
                            repository = repository,
                            onError = { error = it },
                        )
                    }
                }) { Text("Use HTTP") }
            },
            dismissButton = { TextButton({ insecureWarning = false }) { Text("Cancel") } },
        )
    }
}

internal fun shouldConfirmCleartextOrigin(origin: String, allowCleartextTraffic: Boolean): Boolean {
    if (!allowCleartextTraffic) return false
    return origin.trim().startsWith("http://", ignoreCase = true)
}

private suspend fun connect(
    origin: String,
    allowCleartextTraffic: Boolean,
    settingsStore: SettingsStore,
    repository: MomentoRepository,
    onError: (String) -> Unit,
) {
    try {
        val normalized = normalizeServerOrigin(origin, allowCleartextTraffic)
        repository.capabilities(normalized)
        settingsStore.setOrigin(normalized, allowCleartextTraffic)
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
private fun LoginScreen(
    repository: MomentoRepository,
    signedIn: (User) -> Unit,
    passwordChangeRequired: (User) -> Unit,
) {
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
                when (loginRequirement(user)) {
                    LoginRequirement.CHANGE_PASSWORD -> passwordChangeRequired(user)
                    LoginRequirement.COMPLETE_SESSION -> {
                        signedIn(user)
                        repository.completeLogin()
                    }
                }
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

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onBackground,
    ) {
        Column(
            modifier = Modifier.fillMaxSize().imePadding().padding(28.dp),
            verticalArrangement = Arrangement.Center,
        ) {
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

@Composable
private fun ForcedPasswordChangeScreen(
    repository: MomentoRepository,
    username: String,
    passwordChanged: () -> Unit,
) {
    var currentPassword by remember { mutableStateOf("") }
    var newPassword by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var submitting by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun submit() {
        if (submitting) return
        val validation = validateNewPassword(newPassword, confirmation)
        if (validation != null) {
            error = validation
            return
        }

        scope.launch {
            submitting = true
            try {
                repository.changePassword(currentPassword, newPassword)
                passwordChanged()
            } catch (_: HttpException) {
                error = "Could not change password. Check your current password."
            } catch (_: IOException) {
                error = "Could not reach the server"
            } catch (_: kotlinx.serialization.SerializationException) {
                error = "Server returned an invalid response"
            } finally {
                submitting = false
            }
        }
    }

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onBackground,
    ) {
        Column(
            modifier = Modifier.fillMaxSize().imePadding().padding(28.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                "Change your password",
                style = MaterialTheme.typography.displaySmall,
                fontWeight = FontWeight.Bold,
            )
            Text(
                "$username must choose a new password before using Momento.",
                modifier = Modifier.padding(top = 12.dp, bottom = 20.dp),
            )
            PasswordChangeFields(
                currentPassword = currentPassword,
                newPassword = newPassword,
                confirmation = confirmation,
                changeCurrentPassword = { currentPassword = it },
                changeNewPassword = { newPassword = it },
                changeConfirmation = { confirmation = it },
                enabled = !submitting,
                errorMessage = error,
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = { submit() },
                enabled = !submitting,
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
            ) {
                Text(if (submitting) "Changing password" else "Change password")
            }
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

@Composable
private fun MainShell(
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    initialUser: User?,
    userLoaded: (User) -> Unit,
) {
    var destination by remember { mutableStateOf(Destination.TIMELINE) }
    var viewerMedia by remember { mutableStateOf<List<Media>?>(null) }
    var viewerIndex by remember { mutableIntStateOf(0) }
    var contentRevision by remember { mutableIntStateOf(0) }
    var viewerChanged by remember { mutableStateOf(false) }
    var user by remember { mutableStateOf(initialUser) }
    var timelinePeriod by remember { mutableStateOf(TimelinePeriod.DAY) }
    var timelineSearchQuery by rememberSaveable { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val drawerState = rememberDrawerState(DrawerValue.Closed)

    BackHandler(enabled = drawerState.isOpen) { scope.launch { drawerState.close() } }
    BackHandler(enabled = !drawerState.isOpen && destination != Destination.TIMELINE) {
        destination = Destination.TIMELINE
    }
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
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        ModalNavigationDrawer(
            drawerState = drawerState,
            gesturesEnabled = destination != Destination.MAP,
            drawerContent = {
                MainNavigationDrawer(
                    destination = destination,
                    select = { selectedDestination ->
                        scope.launch {
                            drawerState.close()
                            destination = selectedDestination
                        }
                    },
                    logout = {
                        scope.launch {
                            drawerState.close()
                            repository.logout()
                        }
                    },
                )
            },
        ) {
            Box(Modifier.fillMaxSize()) {
                key(contentRevision) {
                    ShellDestination(
                        destination = destination,
                        timelinePeriod = timelinePeriod,
                        timelineSearchQuery = timelineSearchQuery,
                        repository = repository,
                        settingsStore = settingsStore,
                        user = user,
                        openDestination = { destination = it },
                        openMedia = { media, index ->
                            viewerMedia = media
                            viewerIndex = index
                            viewerChanged = false
                        },
                        logout = { scope.launch { repository.logout() } },
                    )
                }
                ShellOverlay(
                    destination = destination,
                    timelinePeriod = timelinePeriod,
                    selectTimelinePeriod = { timelinePeriod = it },
                    openMenu = { scope.launch { drawerState.open() } },
                    search = { query ->
                        timelineSearchQuery = query
                    },
                )
            }
        }
        viewerMedia?.let { media ->
            ViewerScreen(
                media = media,
                initialIndex = viewerIndex,
                repository = repository,
                mediaChanged = { viewerChanged = true },
                close = {
                    viewerMedia = null
                    if (viewerChanged) contentRevision += 1
                },
            )
        }
    }
}

@Composable
private fun MainNavigationDrawer(
    destination: Destination,
    select: (Destination) -> Unit,
    logout: () -> Unit,
) {
    var timelineExpanded by rememberSaveable { mutableStateOf(true) }
    var utilityExpanded by rememberSaveable { mutableStateOf(false) }
    val drawerDestinations = webDrawerDestinations.drop(1 + timelineSubpageDestinations.size)
    val utilityStart = drawerDestinations.indexOf(utilityDrawerDestinations.first())
    ModalDrawerSheet(
        modifier = Modifier.width(280.dp),
        drawerContainerColor = MaterialTheme.colorScheme.background,
        drawerContentColor = MaterialTheme.colorScheme.onBackground,
    ) {
        Column(Modifier.fillMaxSize()) {
            Text(
                "Momento",
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(24.dp),
            )
            LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 12.dp)) {
                item {
                    NavigationDrawerItem(
                        label = { Text(Destination.TIMELINE.label) },
                        selected = destination == Destination.TIMELINE ||
                            (!timelineExpanded && destination.isTimelinePage()),
                        onClick = {
                            if (destination == Destination.TIMELINE) {
                                timelineExpanded = !timelineExpanded
                            } else {
                                select(Destination.TIMELINE)
                            }
                        },
                        icon = { Icon(Icons.Default.PhotoLibrary, null) },
                        badge = {
                            IconButton(onClick = { timelineExpanded = !timelineExpanded }) {
                                Icon(
                                    if (timelineExpanded) Icons.Default.ExpandLess else Icons.Default.ExpandMore,
                                    if (timelineExpanded) "Collapse Timeline" else "Expand Timeline",
                                )
                            }
                        },
                        modifier = Modifier.padding(top = 2.dp, bottom = 2.dp),
                    )
                    AnimatedVisibility(visible = timelineExpanded) {
                        Column {
                            timelineSubpageDestinations.forEach { timelineDestination ->
                                DrawerDestinationItem(
                                    destination = timelineDestination,
                                    selectedDestination = destination,
                                    icon = drawerIcon(timelineDestination),
                                    indentation = 20.dp,
                                    select = select,
                                )
                            }
                        }
                    }
                }
                items(drawerDestinations.take(utilityStart)) { drawerDestination ->
                    DrawerDestinationItem(
                        destination = drawerDestination,
                        selectedDestination = destination,
                        icon = drawerIcon(drawerDestination),
                        indentation = 0.dp,
                        select = select,
                    )
                }
                item {
                    NavigationDrawerItem(
                        label = { Text("Utility") },
                        selected = !utilityExpanded && destination in utilityDrawerDestinations,
                        onClick = { utilityExpanded = !utilityExpanded },
                        icon = { Icon(Icons.Default.Build, null) },
                        badge = {
                            IconButton(onClick = { utilityExpanded = !utilityExpanded }) {
                                Icon(
                                    if (utilityExpanded) Icons.Default.ExpandLess else Icons.Default.ExpandMore,
                                    if (utilityExpanded) "Collapse Utility" else "Expand Utility",
                                )
                            }
                        },
                        modifier = Modifier.padding(top = 2.dp, bottom = 2.dp),
                    )
                    AnimatedVisibility(visible = utilityExpanded) {
                        Column {
                            utilityDrawerDestinations.forEach { utilityDestination ->
                                DrawerDestinationItem(
                                    destination = utilityDestination,
                                    selectedDestination = destination,
                                    icon = drawerIcon(utilityDestination),
                                    indentation = 20.dp,
                                    select = select,
                                )
                            }
                        }
                    }
                }
                items(drawerDestinations.drop(utilityStart + utilityDrawerDestinations.size)) { drawerDestination ->
                    DrawerDestinationItem(
                        destination = drawerDestination,
                        selectedDestination = destination,
                        icon = drawerIcon(drawerDestination),
                        indentation = 0.dp,
                        select = select,
                    )
                }
            }
            HorizontalDivider()
            DrawerDestinationItem(Destination.SETTINGS, destination, Icons.Default.Settings, 0.dp, select)
            NavigationDrawerItem(
                label = { Text("Logout") },
                selected = false,
                onClick = logout,
                icon = { Icon(Icons.AutoMirrored.Filled.Logout, null) },
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun DrawerDestinationItem(
    destination: Destination,
    selectedDestination: Destination,
    icon: ImageVector,
    indentation: Dp,
    select: (Destination) -> Unit,
) {
    NavigationDrawerItem(
        label = { Text(destination.label) },
        selected = destination == selectedDestination,
        onClick = { select(destination) },
        icon = { Icon(icon, null) },
        modifier = Modifier.padding(start = indentation, top = 2.dp, bottom = 2.dp),
    )
}

@Composable
private fun ShellOverlay(
    destination: Destination,
    timelinePeriod: TimelinePeriod,
    selectTimelinePeriod: (TimelinePeriod) -> Unit,
    openMenu: () -> Unit,
    search: (String) -> Unit,
) {
    var searchExpanded by rememberSaveable { mutableStateOf(false) }
    var searchQuery by rememberSaveable { mutableStateOf("") }
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current

    fun changeSearchExpanded(expanded: Boolean) {
        searchExpanded = expanded
        if (expanded) return

        focusManager.clearFocus()
        keyboard?.hide()
    }

    BackHandler(enabled = searchExpanded) { changeSearchExpanded(false) }

    Box(Modifier.fillMaxSize()) {
        if (searchExpanded) {
            Box(
                Modifier
                    .fillMaxSize()
                    .clickable { changeSearchExpanded(false) },
            )
        }

        // Only controls move for the IME; the dismiss surface must cover the whole window.
        Box(
            Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .imePadding(),
        ) {
            if (destination.hasFloatingTitle()) {
                val floatingColors = momentoFloatingControlColors()
                Surface(
                    modifier = Modifier.align(Alignment.TopStart).padding(12.dp),
                    shape = MaterialTheme.shapes.extraLarge,
                    color = floatingColors.container,
                    contentColor = floatingColors.content,
                    shadowElevation = 0.dp,
                    tonalElevation = 0.dp,
                ) {
                    Text(
                        destination.label,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
                    )
                }
            }

            MomentoFloatingButton(
                modifier = Modifier.align(Alignment.BottomStart).padding(12.dp),
                onClick = {
                    changeSearchExpanded(false)
                    openMenu()
                },
            ) {
                Icon(Icons.Default.Menu, "Open navigation menu")
            }

            if (destination.isTimelinePage() && !searchExpanded) {
                TimelinePeriodDock(
                    selected = timelinePeriod,
                    select = selectTimelinePeriod,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
                )
            }
            if (destination.isTimelinePage()) {
                TimelineSearchControl(
                    query = searchQuery,
                    changeQuery = { searchQuery = it },
                    expanded = searchExpanded,
                    changeExpanded = ::changeSearchExpanded,
                    submit = search,
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .fillMaxWidth()
                        .padding(start = 80.dp, end = 12.dp, bottom = 12.dp),
                )
            }
        }
    }
}

@Composable
private fun TimelinePeriodDock(
    selected: TimelinePeriod,
    select: (TimelinePeriod) -> Unit,
    modifier: Modifier,
) {
    val floatingColors = momentoFloatingControlColors()
    Surface(
        modifier = modifier,
        shape = MaterialTheme.shapes.extraLarge,
        color = floatingColors.container,
        contentColor = floatingColors.content,
        shadowElevation = 0.dp,
        tonalElevation = 0.dp,
    ) {
        Row(
            Modifier.padding(4.dp).selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            TimelinePeriod.entries.forEach { period ->
                Box(
                    modifier = Modifier
                        .size(48.dp)
                        .background(
                            color = if (selected == period) floatingColors.selected else Color.Transparent,
                            shape = CircleShape,
                        )
                        .selectable(
                            selected = selected == period,
                            onClick = { select(period) },
                            role = Role.RadioButton,
                        ),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        period.label,
                        style = MaterialTheme.typography.labelMedium,
                        color = floatingColors.content,
                    )
                }
            }
        }
    }
}

@Composable
private fun TimelineSearchControl(
    query: String,
    changeQuery: (String) -> Unit,
    expanded: Boolean,
    changeExpanded: (Boolean) -> Unit,
    submit: (String) -> Unit,
    modifier: Modifier,
) {
    val focusRequester = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    var searchFieldValue by remember { mutableStateOf(TextFieldValue(query)) }
    val shape = CircleShape
    val floatingColors = momentoFloatingControlColors()

    fun runSearch() {
        submit(normalizedTimelineSearchQuery(query))
        changeExpanded(false)
    }

    LaunchedEffect(expanded) {
        if (!expanded) return@LaunchedEffect

        focusRequester.requestFocus()
        keyboard?.show()
    }
    LaunchedEffect(query) {
        if (searchFieldValue.text != query) {
            searchFieldValue = TextFieldValue(query)
        }
    }
    Box(modifier = modifier, contentAlignment = Alignment.CenterEnd) {
        Surface(
            modifier = Modifier
                .animateContentSize()
                .then(if (expanded) Modifier.fillMaxWidth() else Modifier.width(56.dp))
                .height(56.dp),
            shape = shape,
            color = floatingColors.container,
            contentColor = floatingColors.content,
            shadowElevation = 0.dp,
            tonalElevation = 0.dp,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (expanded) {
                    BasicTextField(
                        value = searchFieldValue,
                        onValueChange = { updatedValue ->
                            searchFieldValue = updatedValue
                            changeQuery(updatedValue.text)
                        },
                        modifier = Modifier
                            .weight(1f)
                            .padding(start = 18.dp)
                            .focusRequester(focusRequester)
                            .onFocusChanged { focusState ->
                                if (focusState.isFocused && searchFieldValue.text.isNotEmpty()) {
                                    searchFieldValue = searchFieldValue.copy(
                                        selection = TextRange(0, searchFieldValue.text.length),
                                    )
                                }
                            },
                        textStyle = MaterialTheme.typography.bodyLarge.copy(color = floatingColors.content),
                        cursorBrush = SolidColor(floatingColors.content),
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.Text,
                            imeAction = ImeAction.Search,
                        ),
                        keyboardActions = KeyboardActions(onSearch = { runSearch() }),
                        decorationBox = { innerTextField ->
                            Box {
                                if (query.isEmpty()) {
                                    Text(
                                        "Search photos",
                                        color = floatingColors.content.copy(alpha = 0.7f),
                                    )
                                }
                                innerTextField()
                            }
                        },
                    )
                    IconButton(onClick = { runSearch() }, modifier = Modifier.size(56.dp)) {
                        Icon(Icons.Default.Search, "Search")
                    }
                } else {
                    IconButton(onClick = { changeExpanded(true) }, modifier = Modifier.size(56.dp)) {
                        Icon(Icons.Default.Search, "Open search")
                    }
                }
            }
        }
    }
}

private fun drawerIcon(destination: Destination) = when (destination) {
    Destination.PHOTOS -> Icons.Default.Image
    Destination.VIDEOS -> Icons.Default.Videocam
    Destination.SCREENSHOTS -> Icons.Default.Screenshot
    Destination.DOCUMENTS -> Icons.Default.Description
    Destination.ALBUMS -> Icons.Default.Folder
    Destination.MAP -> Icons.Default.Map
    Destination.PLACES -> Icons.Default.Place
    Destination.FACES -> Icons.Default.Face
    Destination.DEDUPLICATE -> Icons.Default.ContentCopy
    Destination.TRASH -> Icons.Default.Delete
    else -> Icons.Default.Folder
}

@Composable
private fun ShellDestination(
    destination: Destination,
    timelinePeriod: TimelinePeriod,
    timelineSearchQuery: String,
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    user: User?,
    openDestination: (Destination) -> Unit,
    openMedia: (List<Media>, Int) -> Unit,
    logout: () -> Unit,
) {
    when (destination) {
        Destination.TIMELINE,
        Destination.PHOTOS,
        Destination.VIDEOS,
        Destination.SCREENSHOTS,
        Destination.DOCUMENTS -> TimelineScreen(
            repository = repository,
            page = timelinePage(destination),
            period = timelinePeriod,
            search = timelineSearchQuery,
            openMedia = openMedia,
        )
        Destination.COLLECTIONS -> CollectionsScreen(openDestination)
        Destination.SETTINGS -> SettingsScreen(repository, settingsStore, user, { openDestination(Destination.ADMIN) }, logout)
        Destination.ALBUMS -> AlbumsScreen(repository, openMedia)
        Destination.MAP -> NativeMapScreen(repository) { media -> openMedia(media, 0) }
        Destination.PLACES -> PlacesScreen(repository, openMedia)
        Destination.FACES -> FacesScreen(repository, user?.role == "admin", openMedia)
        Destination.DEDUPLICATE -> DeduplicateScreen(repository, user?.role == "admin", openMedia)
        Destination.TRASH -> TrashScreen(repository)
        Destination.ADMIN -> AdminScreen(repository, settingsStore)
        Destination.VIEWER -> Unit
    }
}

private fun timelinePage(destination: Destination): TimelinePage = when (destination) {
    Destination.TIMELINE -> TimelinePage.TIMELINE
    Destination.PHOTOS -> TimelinePage.PHOTOS
    Destination.VIDEOS -> TimelinePage.VIDEOS
    Destination.SCREENSHOTS -> TimelinePage.SCREENSHOTS
    Destination.DOCUMENTS -> TimelinePage.DOCUMENTS
    else -> error("Destination ${destination.name} is not a timeline page")
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
