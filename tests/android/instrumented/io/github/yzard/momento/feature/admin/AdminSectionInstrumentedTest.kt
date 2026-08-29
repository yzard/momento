package io.github.yzard.momento.feature.admin

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollTo
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.User
import org.junit.Rule
import org.junit.Test

class AdminSectionInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun statusMetricsAlwaysExposeTheRequestedImportFields() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                AdminStatusMetrics(
                    listOf(
                        AdminMetric("Status", "idle", false),
                        AdminMetric("Imported", "0", false),
                        AdminMetric("Failed", "0", false),
                        AdminMetric("Total Media", "5103", false),
                    ),
                )
            }
        }

        listOf("STATUS", "IMPORTED", "FAILED", "TOTAL MEDIA").forEach { label ->
            composeRule.onNodeWithText(label).assertIsDisplayed()
        }
        composeRule.onNodeWithText("5103").assertIsDisplayed()
    }

    @Test
    fun aiControlsExposeOneTableAndASelectableFailureLogBelowIt() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                androidx.compose.foundation.layout.Column {
                    AiControlTable(
                        status = null,
                        busyControls = emptySet(),
                        primary = { _, _, _ -> },
                        clean = { _, _ -> },
                        save = { _, _, _ -> },
                    )
                    AdminFailureLog("AI failure log", listOf("[OCR] decode failed"))
                }
            }
        }

        listOf("Feature", "Minute", "Hour", "Day", "Month", "Weekday", "Save", "Start / Cancel", "Clean")
            .forEach { label -> composeRule.onAllNodesWithText(label).assertCountEquals(1) }
        composeRule.onNodeWithText("AI FAILURE LOG").assertIsDisplayed()
        composeRule.onNodeWithText("[OCR] decode failed").assertIsDisplayed()
    }

    @Test
    fun usersRenderAsRowsAndReservedAdminActionsAreDisabled() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                UsersTable(
                    users = listOf(
                        user(2, "admin", true),
                        user(7, "member", false),
                    ),
                    currentUserId = 1,
                    busyUserIds = emptySet(),
                    changeRole = { _, _ -> },
                    toggleActive = {},
                    delete = {},
                )
            }
        }

        composeRule.onNodeWithText("admin").assertIsDisplayed()
        composeRule.onNodeWithText("member").assertIsDisplayed()
        composeRule.onNodeWithText("RESERVED").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Delete admin").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("Delete member").performScrollTo().assertIsDisplayed()
    }

    private fun user(id: Long, username: String, reserved: Boolean) = User(
        id = id,
        username = username,
        email = "$username@example.com",
        role = if (reserved) "admin" else "user",
        isReserved = reserved,
        mustChangePassword = false,
        isActive = true,
        createdAt = "2026-01-01T00:00:00Z",
    )
}
