package io.github.yzard.momento.feature.albums

import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class AlbumDetailActionsInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun primaryActionsStayInSelectEditDeleteOrderAndInvokeTheirAction() {
        val clicks = mutableListOf<String>()
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                AlbumPrimaryActionDock(
                    enabled = true,
                    select = { clicks += "Select" },
                    edit = { clicks += "Edit" },
                    delete = { clicks += "Delete" },
                    modifier = Modifier,
                )
            }
        }

        val select = composeRule.onNodeWithText("Select").assertHasClickAction()
        val edit = composeRule.onNodeWithText("Edit").assertHasClickAction()
        val delete = composeRule.onNodeWithText("Delete").assertHasClickAction()
        assertTrue(select.fetchSemanticsNode().boundsInRoot.left < edit.fetchSemanticsNode().boundsInRoot.left)
        assertTrue(edit.fetchSemanticsNode().boundsInRoot.left < delete.fetchSemanticsNode().boundsInRoot.left)

        select.performClick()
        edit.performClick()
        delete.performClick()
        composeRule.runOnIdle { assertEquals(listOf("Select", "Edit", "Delete"), clicks) }
    }
}
