package io.github.yzard.momento.app.navigation

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.app.designsystem.MomentoFloatingDock
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.feature.timeline.TimelinePeriod
import io.github.yzard.momento.feature.timeline.normalizedTimelineSearchQuery

@Composable
internal fun ShellOverlay(
    destination: Destination,
    timelinePeriod: TimelinePeriod,
    searchQuery: String,
    selectTimelinePeriod: (TimelinePeriod) -> Unit,
    openMenu: () -> Unit,
    search: (String) -> Unit,
) {
    var editing by rememberSaveable { mutableStateOf(false) }
    var draft by rememberSaveable(searchQuery, stateSaver = TextFieldValue.Saver) {
        mutableStateOf(TextFieldValue(searchQuery))
    }
    val activeSearch = searchQuery.isNotBlank()
    val timeline = destination.isTimelinePage()
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    val focusRequester = remember { FocusRequester() }
    val floatingColors = momentoFloatingControlColors()

    fun hideKeyboard() {
        editing = false
        focusManager.clearFocus()
        keyboard?.hide()
    }
    fun dismissEditing() {
        draft = TextFieldValue(searchQuery)
        hideKeyboard()
    }
    fun submitSearch() {
        val query = normalizedTimelineSearchQuery(draft.text)
        search(query)
        draft = TextFieldValue(query)
        hideKeyboard()
    }
    fun clearSearch() {
        search("")
        draft = TextFieldValue("")
        hideKeyboard()
    }

    BackHandler(enabled = timeline && editing) { dismissEditing() }
    LaunchedEffect(editing, timeline) {
        if (editing && timeline) {
            focusRequester.requestFocus()
            keyboard?.show()
        }
    }

    Box(Modifier.fillMaxSize()) {
        if (timeline && editing) {
            Box(Modifier.fillMaxSize().testTag("dismiss-search-keyboard").clickable { dismissEditing() })
        }
        Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).imePadding()) {
            MomentoFloatingButton(
                modifier = Modifier.align(Alignment.BottomStart).padding(12.dp),
                onClick = { dismissEditing(); openMenu() },
            ) { Icon(Icons.Default.Menu, "Open navigation menu") }

            if (timeline && !activeSearch && !editing) {
                TimelinePeriodDock(
                    selected = timelinePeriod,
                    select = selectTimelinePeriod,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
                )
            }
            if (timeline && (activeSearch || editing)) {
                Surface(
                    modifier = Modifier.align(Alignment.BottomCenter)
                        .fillMaxWidth().padding(start = 80.dp, end = 80.dp, bottom = 12.dp)
                        .height(56.dp).testTag("timeline-search-bar"),
                    shape = CircleShape,
                    color = floatingColors.container,
                    contentColor = floatingColors.content,
                    shadowElevation = 0.dp,
                    tonalElevation = 0.dp,
                ) {
                    Box(Modifier.padding(horizontal = 18.dp), contentAlignment = Alignment.CenterStart) {
                        BasicTextField(
                            value = draft,
                            onValueChange = { draft = it },
                            modifier = Modifier.fillMaxWidth().focusRequester(focusRequester)
                                .onFocusChanged { if (it.isFocused) editing = true }
                                .semantics { contentDescription = "Search photos" },
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = floatingColors.content),
                            cursorBrush = SolidColor(floatingColors.content),
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text, imeAction = ImeAction.Search),
                            keyboardActions = KeyboardActions(onSearch = { submitSearch() }),
                            decorationBox = { innerTextField ->
                                Box {
                                    if (draft.text.isEmpty()) Text("Search photos", color = floatingColors.content.copy(alpha = 0.7f))
                                    innerTextField()
                                }
                            },
                        )
                    }
                }
            }
            if (timeline) {
                MomentoFloatingButton(
                    modifier = Modifier.align(Alignment.BottomEnd).padding(12.dp),
                    onClick = {
                        when {
                            activeSearch -> clearSearch()
                            editing -> submitSearch()
                            else -> editing = true
                        }
                    },
                ) {
                    if (activeSearch) Icon(Icons.Default.Close, "Clear search")
                    else Icon(Icons.Default.Search, if (editing) "Search" else "Open search")
                }
            }
        }
    }
}

@Composable
private fun TimelinePeriodDock(selected: TimelinePeriod, select: (TimelinePeriod) -> Unit, modifier: Modifier) {
    val colors = momentoFloatingControlColors()
    MomentoFloatingDock(modifier = modifier.selectableGroup().testTag("timeline-period-dock")) {
        TimelinePeriod.entries.forEach { period ->
            Box(
                modifier = Modifier.size(48.dp)
                    .background(if (selected == period) colors.selected else Color.Transparent, CircleShape)
                    .selectable(selected = selected == period, onClick = { select(period) }, role = Role.RadioButton),
                contentAlignment = Alignment.Center,
            ) { Text(period.label, style = MaterialTheme.typography.labelMedium, color = colors.content) }
        }
    }
}
