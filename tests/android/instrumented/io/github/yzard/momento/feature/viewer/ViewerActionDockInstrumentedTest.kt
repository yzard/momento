package io.github.yzard.momento.feature.viewer

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoFloatingButton
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.app.designsystem.momentoFloatingControlHeight
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class ViewerActionDockInstrumentedTest {
    @get:Rule val rule = createComposeRule()
    @Test fun lightViewerActionsMatchTheOtherFloatingButtons() = verify(ThemePreference.LIGHT)
    @Test fun darkViewerActionsMatchTheOtherFloatingButtons() = verify(ThemePreference.DARK)

    private fun verify(theme: ThemePreference) {
        var albums = 0
        var information = 0
        rule.setContent {
            MomentoTheme(theme) {
                Row(Modifier.background(MaterialTheme.colorScheme.background), horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                    MomentoFloatingButton(Modifier.testTag("reference"), {}) { Icon(Icons.Default.Share, "Share") }
                    ViewerActionDock({ albums++ }, { information++ }, Modifier.testTag("actions"))
                }
            }
        }
        for (tag in listOf("reference", "actions")) rule.onNodeWithTag(tag).assertHeightIsEqualTo(momentoFloatingControlHeight)
        val reference = rule.onNodeWithTag("reference").captureToImage().toPixelMap()
        val actions = rule.onNodeWithTag("actions").captureToImage().toPixelMap()
        // Sample blank fill and the top border, outside either icon's bounds.
        for (y in listOf(1, reference.height / 5)) {
            val expected = reference[reference.width / 2, y]
            val actual = actions[actions.width / 2, y]
            assertColorMatches(expected, actual)
        }
        rule.onNodeWithContentDescription("Add to album").performClick()
        rule.onNodeWithContentDescription("Information").performClick()
        rule.runOnIdle { assertEquals(1, albums); assertEquals(1, information) }
    }
    private fun assertColorMatches(expected: Color, actual: Color) {
        assertEquals(expected.red, actual.red, 0.08f)
        assertEquals(expected.green, actual.green, 0.08f)
        assertEquals(expected.blue, actual.blue, 0.08f)
    }
}
