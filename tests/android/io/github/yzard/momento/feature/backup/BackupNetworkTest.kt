package io.github.yzard.momento.feature.backup

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupNetworkTest {
    @Test fun backupRequiresValidatedInternetAndHonorsMetering() {
        assertFalse(backupNetworkAllowed(true, hasValidatedInternet = false, unmetered = true))
        assertFalse(backupNetworkAllowed(false, hasValidatedInternet = true, unmetered = false))
        assertTrue(backupNetworkAllowed(false, hasValidatedInternet = true, unmetered = true))
        assertTrue(backupNetworkAllowed(true, hasValidatedInternet = true, unmetered = false))
    }

    @Test fun retryPolicyKeepsOnlyTransientHttpFailures() {
        assertTrue(isRetryable(408))
        assertTrue(isRetryable(429))
        assertTrue(isRetryable(503))
        assertFalse(isRetryable(400))
    }
}
