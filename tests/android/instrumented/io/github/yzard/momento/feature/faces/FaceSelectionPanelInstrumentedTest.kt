package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Face
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertWidthIsEqualTo
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Rule
import org.junit.Test

class FaceSelectionPanelInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    private fun showPanel(width: Int, working: Boolean, fontScale: Float) {
        composeRule.setContent {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density / 2, fontScale)) {
                MomentoTheme(ThemePreference.DARK) {
                    var ids by remember { mutableStateOf(listOf(1L, 2L, 3L)) }
                    Box(Modifier.requiredWidth(width.dp)) {
                        FaceSelectionPanel(
                            selectedIds = ids,
                            working = working,
                            actions = listOf(
                                MomentoSelectionAction("Not a face", Icons.Default.Face, !working, true, {}),
                                MomentoSelectionAction("Merge", Icons.Default.Face, !working && ids.size > 1, false, {}),
                            ),
                            clearSelection = { ids = listOf(1L) },
                            modifier = Modifier.testTag("panel"),
                        )
                    }
                }
            }
        }
    }

    @Test fun portraitLargeTextKeepsActionsAndSelectionTogether() {
        showPanel(320, false, 1.4f)
        composeRule.onNodeWithText("Not a face").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Merge").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("3 selected").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Clear selection").performScrollTo().performClick()
        composeRule.onNodeWithText("1 selected").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Merge").performScrollTo().assertIsNotEnabled()
    }

    @Test fun unfoldedDockKeepsTheSelectionCountVisible() {
        showPanel(760, false, 1f)
        composeRule.onNodeWithText("3 selected").performScrollTo().assertIsDisplayed()
    }

    @Test fun pendingOperationDisablesSelectionChanges() {
        showPanel(360, true, 1f)
        composeRule.onNodeWithContentDescription("Clear selection").assertIsNotEnabled()
        composeRule.onNodeWithText("Not a face").assertIsNotEnabled()
        composeRule.onNodeWithText("Merge").assertIsNotEnabled()
    }
}
