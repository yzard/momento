package io.github.yzard.momento.core.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AuthenticatedImageSourceTest {
    @Test
    fun `square image request uses one explicit target size`() {
        val spec = squareAuthenticatedImageSpec("thumbnail", 96, allowHardware = false)

        assertEquals(96, spec.widthPx)
        assertEquals(96, spec.heightPx)
        assertEquals(false, spec.allowHardware)
    }

    @Test
    fun `image request rejects invalid dimensions`() {
        assertThrows(IllegalArgumentException::class.java) {
            squareAuthenticatedImageSpec("thumbnail", 0, allowHardware = false)
        }
    }
}
