package io.github.yzard.momento.core.data

import org.junit.Assert.assertEquals
import org.junit.Test

class SettingsStoreTest {
    @Test fun normalizesHttpsOrigin() = assertEquals(
        "https://photos.example.com",
        normalizeServerOrigin(" https://photos.example.com/ ", allowCleartextTraffic = false),
    )

    @Test fun debugBuildAcceptsHttpOrigin() = assertEquals(
        "http://10.0.2.2:8000",
        normalizeServerOrigin("http://10.0.2.2:8000/", allowCleartextTraffic = true),
    )

    @Test(expected = IllegalArgumentException::class)
    fun releaseBuildRejectsHttpOrigin() {
        normalizeServerOrigin("http://photos.example.com", allowCleartextTraffic = false)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsOriginPath() {
        normalizeServerOrigin("https://photos.example.com/api", allowCleartextTraffic = false)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsSchemeLessOrigin() {
        normalizeServerOrigin("photos.example.com", allowCleartextTraffic = false)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsCredentialsInOrigin() {
        normalizeServerOrigin("https://user:password@photos.example.com", allowCleartextTraffic = false)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsQueryInOrigin() {
        normalizeServerOrigin("https://photos.example.com?token=secret", allowCleartextTraffic = false)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsMalformedOrigin() {
        normalizeServerOrigin("https://photos example.com", allowCleartextTraffic = false)
    }
    @Test fun missingThemeFollowsSystem() { assertEquals(ThemePreference.SYSTEM, parseThemePreference(null)) }
    @Test fun storedThemeIsParsed() { assertEquals(ThemePreference.DARK, parseThemePreference("DARK")) }
    @Test fun invalidThemeFollowsSystem() { assertEquals(ThemePreference.SYSTEM, parseThemePreference("sepia")) }

    @Test fun backupGenerationsAreServerSafeAndUnique() {
        val first = newBackupGeneration()
        val second = newBackupGeneration()
        assertEquals(true, first.matches(Regex("^[a-f0-9]{32}$")))
        assertEquals(false, first == second)
    }

    @Test
    fun incompleteAuthenticationDoesNotSurviveAProcessRestart() {
        assertEquals(false, authenticationCompleted("encrypted-token", storedCompletion = false))
        assertEquals(true, authenticationCompleted("encrypted-token", storedCompletion = true))
        assertEquals(false, authenticationCompleted(null, storedCompletion = true))
    }

    @Test
    fun existingInstallTokensRemainAuthenticatedDuringMigration() {
        assertEquals(true, authenticationCompleted("encrypted-token", storedCompletion = null))
    }
}
