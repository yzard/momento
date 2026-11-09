package io.github.yzard.momento.feature.settings

import okhttp3.ResponseBody.Companion.toResponseBody
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.assertIsDisplayed
import org.junit.Assert.assertTrue
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

    @Test fun clearIsLeftOfTheChooserAndRemovesCachedMediaWithoutChangingItsLimit() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val store = MediaCacheStore.get(context)
        val previous = store.limit.value
        try {
            runBlocking { store.setLimit(MediaCacheLimit.MIB_512) }
            val client = okhttp3.OkHttpClient.Builder().addInterceptor(store.interceptor { "clear-fixture" })
                .addInterceptor { chain ->
                    okhttp3.Response.Builder().request(chain.request()).protocol(okhttp3.Protocol.HTTP_1_1)
                        .code(200).message("Fixture").body("cached thumbnail".toResponseBody()).build()
                }.build()
            client.newCall(okhttp3.Request.Builder().url("https://momento.example/api/v1/media/1/thumbnail").build()).execute().use { it.body!!.string() }
            val directory = java.io.File(context.cacheDir, "media-cache-v1")
            assertTrue(directory.listFiles().orEmpty().any { it.extension == "data" })
            composeRule.setContent { MomentoTheme(ThemePreference.DARK) { MediaCacheSettingsSection() } }
            val clear = composeRule.onNodeWithText("Clear").fetchSemanticsNode().boundsInRoot
            val choice = composeRule.onNodeWithText("512 MiB").fetchSemanticsNode().boundsInRoot
            assertTrue(clear.right <= choice.left)
            assertTrue(kotlin.math.abs(clear.center.y - choice.center.y) < 2f)
            composeRule.onNodeWithText("Clear").performClick()
            composeRule.waitUntil { directory.listFiles().orEmpty().none { it.extension in setOf("data", "meta", "part") } }
            assertEquals(MediaCacheLimit.MIB_512, store.limit.value)
            composeRule.onNodeWithText("512 MiB").assertIsDisplayed()
        } finally { runBlocking { store.setLimit(previous) } }
    }

    @Test fun selectingCacheLimitAppliesAndPersistsTheChoice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val store = MediaCacheStore.get(context)
        val previous = store.limit.value
        try {
            runBlocking { store.setLimit(MediaCacheLimit.MIB_512) }
            composeRule.setContent { MomentoTheme(ThemePreference.LIGHT) { MediaCacheSettingsSection() } }
            val title = composeRule.onNodeWithText("Local Cache Limit").fetchSemanticsNode().boundsInRoot
            val choice = composeRule.onNodeWithText("512 MiB").fetchSemanticsNode().boundsInRoot
            assertTrue(choice.left > title.right)
            assertTrue(kotlin.math.abs(choice.center.y - title.center.y) < 2f)
            composeRule.onNodeWithText("512 MiB").performClick()
            composeRule.onNodeWithText("256 MiB").performClick()
            composeRule.waitUntil { store.limit.value == MediaCacheLimit.MIB_256 }
            composeRule.onNodeWithText("256 MiB").assertIsDisplayed()
            assertEquals("MIB_256", context.getSharedPreferences("momento_media_cache", 0).getString("limit", null))
        } finally { runBlocking { store.setLimit(previous) } }
    }
}
