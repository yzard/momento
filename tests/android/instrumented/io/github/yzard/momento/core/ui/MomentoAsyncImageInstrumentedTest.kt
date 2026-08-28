package io.github.yzard.momento.core.ui

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.foundation.layout.size
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.network.NetworkClient
import org.junit.Rule
import org.junit.Test

class MomentoAsyncImageInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun contentDescriptionRemainsAvailableWhenImageFails() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                MomentoAsyncImage(
                    model = null,
                    repository = testRepository(),
                    contentDescription = "Memory preview",
                    contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                    modifier = androidx.compose.ui.Modifier.size(80.dp),
                )
            }
        }

        composeRule.onNodeWithContentDescription("Memory preview").fetchSemanticsNode()
    }

    private fun testRepository(): MomentoRepository {
        val context = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        val tokenStore = EncryptedTokenStore(context)
        return MomentoRepository(SettingsStore(context), tokenStore, NetworkClient(tokenStore))
    }
}
