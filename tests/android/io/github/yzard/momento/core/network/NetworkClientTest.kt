package io.github.yzard.momento.core.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NetworkClientTest {
    @Test fun buildsBasicAuthenticationHeader() { assertEquals("Basic YWRtaW46c2VjcmV0", basicAuthorization("admin", "secret")) }
    @Test fun preservesExplicitAuthorization() { assertEquals("Basic abc", authorizationHeader("Basic abc", "access")) }
    @Test fun suppliesBearerOnlyWhenAuthorizationIsAbsent() { assertEquals("Bearer access", authorizationHeader(null, "access")) }
    @Test fun omitsAuthorizationWithoutAnyCredential() { assertNull(authorizationHeader(null, null)) }
}
