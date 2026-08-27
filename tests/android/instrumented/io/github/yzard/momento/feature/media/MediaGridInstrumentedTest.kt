package io.github.yzard.momento.feature.media

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToNode
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Rule
import org.junit.Test

class MediaGridInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun longGridScrollsToTheFinalMediaEntry() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                LazyMediaGrid(
                    entries = (1..100).toList(),
                    entryKey = { mediaId -> mediaId },
                    entrySelected = { false },
                    contentPadding = PaddingValues(),
                    headerContent = { Text("Album header") },
                    footerContent = { Text("Album footer") },
                    modifier = Modifier.testTag("media-grid"),
                ) { mediaId, _ ->
                    Text(mediaId.toString(), Modifier.aspectRatio(1f))
                }
            }
        }

        composeRule.onNodeWithText("Album header").assertIsDisplayed()
        composeRule.onNodeWithTag("media-grid").performScrollToNode(hasText("100"))
        composeRule.onNodeWithText("100").assertIsDisplayed()
    }
}
