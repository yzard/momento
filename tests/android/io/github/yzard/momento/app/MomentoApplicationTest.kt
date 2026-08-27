package io.github.yzard.momento.app

import io.github.yzard.momento.core.model.User
import io.github.yzard.momento.feature.auth.LoginRequirement
import io.github.yzard.momento.feature.auth.loginRequirement
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MomentoApplicationTest {
    @Test
    fun debugBuildConfirmsCleartextOrigin() {
        assertTrue(shouldConfirmCleartextOrigin(" http://10.0.2.2:8000 ", allowCleartextTraffic = true))
    }

    @Test
    fun releaseBuildDoesNotOfferCleartextOverride() {
        assertFalse(shouldConfirmCleartextOrigin("http://photos.example.com", allowCleartextTraffic = false))
        assertFalse(shouldConfirmCleartextOrigin("https://photos.example.com", allowCleartextTraffic = true))
    }

    @Test
    fun forcedPasswordChangePreventsCompletingLogin() {
        assertEquals(
            LoginRequirement.CHANGE_PASSWORD,
            loginRequirement(user(mustChangePassword = true)),
        )
    }

    @Test
    fun normalUserCompletesLogin() {
        assertEquals(
            LoginRequirement.COMPLETE_SESSION,
            loginRequirement(user(mustChangePassword = false)),
        )
    }

    private fun user(mustChangePassword: Boolean): User = User(
        id = 1,
        username = "user",
        email = "user@example.com",
        role = "user",
        mustChangePassword = mustChangePassword,
        isActive = true,
        createdAt = "2026-01-01T00:00:00Z",
    )
}
