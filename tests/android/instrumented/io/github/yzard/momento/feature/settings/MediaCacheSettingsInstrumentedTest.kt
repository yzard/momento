package io.github.yzard.momento.feature.settings

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.cache.MediaCacheLimit
import io.github.yzard.momento.core.cache.MediaCacheStore
import io.github.yzard.momento.core.data.ThemePreference
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class MediaCacheSettingsInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    @Test fun selectingCacheLimitAppliesAndPersistsTheChoice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val store = MediaCacheStore.get(context)
        val previous = store.limit.value
        try {
            runBlocking { store.setLimit(MediaCacheLimit.MIB_512) }
            composeRule.setContent { MomentoTheme(ThemePreference.LIGHT) { MediaCacheSettingsSection() } }
            composeRule.onNodeWithText("Media cache").performClick()
            composeRule.onNodeWithText("256 MiB").performClick()
            composeRule.waitUntil { store.limit.value == MediaCacheLimit.MIB_256 }
            assertEquals("MIB_256", context.getSharedPreferences("momento_media_cache", 0).getString("limit", null))
        } finally { runBlocking { store.setLimit(previous) } }
    }
}
