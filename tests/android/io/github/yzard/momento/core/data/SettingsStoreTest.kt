package io.github.yzard.momento.core.data

import org.junit.Assert.assertEquals
import org.junit.Test

class SettingsStoreTest {
    @Test fun normalizesHttpsOrigin() = assertEquals("https://photos.example.com", normalizeServerOrigin(" https://photos.example.com/ "))
    @Test(expected = IllegalArgumentException::class) fun rejectsOriginPath() { normalizeServerOrigin("https://photos.example.com/api") }
    @Test(expected = IllegalArgumentException::class) fun rejectsSchemeLessOrigin() { normalizeServerOrigin("photos.example.com") }
    @Test fun missingThemeFollowsSystem() { assertEquals(ThemePreference.SYSTEM, parseThemePreference(null)) }
    @Test fun storedThemeIsParsed() { assertEquals(ThemePreference.DARK, parseThemePreference("DARK")) }
    @Test fun invalidThemeFollowsSystem() { assertEquals(ThemePreference.SYSTEM, parseThemePreference("sepia")) }
}
