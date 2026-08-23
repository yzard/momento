package io.github.yzard.momento.core.data

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthenticationStateTest {
    @Test fun exposesLogoutImmediately() {
        val state = AuthenticationState(true)
        assertTrue(state.isAuthenticated.value)
        state.signedOut()
        assertFalse(state.isAuthenticated.value)
    }

    @Test fun exposesLoginOnlyWhenSessionIsComplete() {
        val state = AuthenticationState(false)
        assertFalse(state.isAuthenticated.value)
        state.signedIn()
        assertTrue(state.isAuthenticated.value)
    }
}
