package io.github.yzard.momento.feature.places

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.Place
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class PlaceCardInstrumentedTest {
    @get:Rule val rule = createComposeRule()
    private val place = Place("opaque", "Corona", "New York", "United States", 2837)
    @Test fun compactCardShowsOnlyTheCountAndWrapsCountryOntoItsOwnLine() {
        render(180)
        val layout = regionLayout()
        assertEquals(2, layout.lineCount)
        assertFalse("Region overflow: width=${layout.didOverflowWidth}, height=${layout.didOverflowHeight}, size=${layout.size}", layout.hasVisualOverflow)
        assertEquals("New York,", layout.layoutInput.text.text.substring(0, layout.getLineEnd(0)).trim())
        assertEquals("United\u00a0States", layout.layoutInput.text.text.substring(layout.getLineStart(1)).trim())
        rule.onNodeWithText("2837").assertIsDisplayed()
        rule.onAllNodesWithText("media", substring = true).assertCountEquals(0)
        val badge = rule.onNodeWithText("2837").fetchSemanticsNode().boundsInRoot
        val card = rule.onNodeWithTag("card").fetchSemanticsNode().boundsInRoot
        val rightInset = with(rule.density) { 21.dp.toPx() }
        assertEquals(card.right - rightInset, badge.right, 2f)
    }
    @Test fun wideCardKeepsTheCompleteRegionOnOneLineWhenItFits() {
        render(320)
        val layout = regionLayout()
        assertEquals(1, layout.lineCount)
        assertFalse(layout.didOverflowHeight)
        assertFalse(layout.isLineEllipsized(0))
        assertEquals(placeCardRegion(place).length, layout.getLineEnd(0))
        // Text's intrinsic width can round down by a fraction of a physical pixel.
        assertTrue(layout.getLineRight(0) <= layout.size.width + 1f)
    }
    private fun render(width: Int) {
        rule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                Box(Modifier.fillMaxSize()) {
                    Box(Modifier.size(width.dp, 140.dp).testTag("card")) { PlaceCardOverlay(place) }
                }
            }
        }
    }
    private fun regionLayout(): TextLayoutResult {
        val layouts = mutableListOf<TextLayoutResult>()
        rule.onNodeWithText(placeCardRegion(place)).performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
        return layouts.single()
    }
}
