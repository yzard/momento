package io.github.yzard.momento.app.navigation

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import io.github.yzard.momento.app.designsystem.SelectionDockState
import io.github.yzard.momento.app.designsystem.LocalSelectionDock
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.app.designsystem.PublishSelectionDock
import io.github.yzard.momento.app.designsystem.MomentoSelectionDock
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.feature.timeline.TimelinePeriod
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import kotlin.math.abs

class SelectionDockInstrumentedTest {
    @get:Rule val rule = createComposeRule()

    @Test fun selectionReplacesTimelineControlsBesideTheMenuAndClears() {
        val dock = SelectionDockState()
        rule.setContent {
            var selected by remember { mutableStateOf(true) }
            MomentoTheme(ThemePreference.DARK) {
                CompositionLocalProvider(LocalSelectionDock provides dock) {
                    Box(Modifier.fillMaxSize()) {
                        if (selected) PublishSelectionDock {
                            MomentoSelectionDock(1, listOf(MomentoSelectionAction("Trash", Icons.Default.Delete, true, true, {})), { selected = false }, true, Modifier)
                        }
                        ShellOverlay(Destination.TIMELINE, TimelinePeriod.DAY, "", {}, {}, {})
                    }
                }
            }
        }
        val menu = rule.onNodeWithContentDescription("Open navigation menu").fetchSemanticsNode().boundsInRoot
        val count = rule.onNodeWithText("1 selected").fetchSemanticsNode().boundsInRoot
        val actions = rule.onNodeWithTag("shell-selection-actions").fetchSemanticsNode().boundsInRoot
        val root = rule.onRoot().fetchSemanticsNode().boundsInRoot
        val inset = with(rule.density) { 12.dp.toPx() }
        assertTrue(abs(actions.right - (root.right - inset)) < 3)
        assertTrue(count.left >= menu.right)
        assertTrue(abs(count.center.y - menu.center.y) < 3)
        rule.onNodeWithTag("timeline-period-dock").assertDoesNotExist()
        rule.onNodeWithContentDescription("Clear selection").performClick()
        rule.onNodeWithText("1 selected").assertDoesNotExist()
        rule.onNodeWithTag("timeline-period-dock").assertIsDisplayed()
    }
}
