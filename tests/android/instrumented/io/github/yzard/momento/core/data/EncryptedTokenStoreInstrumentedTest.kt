package io.github.yzard.momento.core.data

import androidx.test.platform.app.InstrumentationRegistry
import io.github.yzard.momento.core.model.TokenPair
import org.junit.After
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class EncryptedTokenStoreInstrumentedTest {
    private lateinit var tokenStore: EncryptedTokenStore

    @Before
    fun prepareTokenStore() {
        tokenStore = EncryptedTokenStore(InstrumentationRegistry.getInstrumentation().targetContext)
        tokenStore.clear()
    }

    @After
    fun clearTokenStore() {
        tokenStore.clear()
    }

    @Test
    fun refreshedTokensKeepAnAuthenticatedSessionSignedIn() {
        tokenStore.saveLoginTokens(TokenPair("access-1", "refresh-1", "Bearer"))
        assertFalse(tokenStore.isAuthenticated.value)
        tokenStore.markAuthenticated()

        tokenStore.replaceSessionTokens(TokenPair("access-2", "refresh-2", "Bearer"))

        assertTrue(tokenStore.isAuthenticated.value)
        assertEquals("access-2", tokenStore.accessToken())
        assertEquals("refresh-2", tokenStore.refreshToken())
    }

    @Test
    fun refreshedTokensDoNotCompleteAnIncompleteLogin() {
        tokenStore.saveLoginTokens(TokenPair("access-1", "refresh-1", "Bearer"))

        tokenStore.replaceSessionTokens(TokenPair("access-2", "refresh-2", "Bearer"))

        assertFalse(tokenStore.isAuthenticated.value)
    }
    @Test
    fun mediaCacheNamespaceSurvivesRefreshButChangesForANewLogin() {
        tokenStore.saveLoginTokens(TokenPair("access-1", "refresh-1", "Bearer"))
        val namespace = tokenStore.mediaCacheNamespace()
        assertNotNull(namespace)
        tokenStore.replaceSessionTokens(TokenPair("access-2", "refresh-2", "Bearer"))
        assertEquals(namespace, tokenStore.mediaCacheNamespace())
        val reopened = EncryptedTokenStore(InstrumentationRegistry.getInstrumentation().targetContext)
        assertEquals(namespace, reopened.mediaCacheNamespace())
        tokenStore.clear()
        assertNull(tokenStore.mediaCacheNamespace())
        tokenStore.saveLoginTokens(TokenPair("access-3", "refresh-3", "Bearer"))
        assertNotEquals(namespace, tokenStore.mediaCacheNamespace())
    }
}
