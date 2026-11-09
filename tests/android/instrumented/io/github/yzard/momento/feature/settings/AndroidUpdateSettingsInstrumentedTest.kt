package io.github.yzard.momento.feature.settings

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.AndroidUpdateRepository
import io.github.yzard.momento.core.data.ThemePreference
import java.io.File
import org.junit.Rule
import org.junit.Test

class AndroidUpdateSettingsInstrumentedTest {
    @get:Rule val rule = createComposeRule()

    @Test fun clientRowShowsOnePlatformVersionWithoutAnInitialDescription() {
        val repository = object : AndroidUpdateRepository {
            override suspend fun downloadAndroidApk(destination: File) =
                error("Layout verification must not download an update")
        }
        rule.setContent { MomentoTheme(ThemePreference.DARK) { AndroidUpdateSettingsSection(repository) } }
        rule.onAllNodesWithText("Momento Android Client ${BuildConfig.VERSION_NAME}").assertCountEquals(1)
        rule.onNodeWithText("Check for update").assertIsDisplayed()
        rule.onAllNodesWithText("Check the signed-in host", substring = true).assertCountEquals(0)
    }
}
