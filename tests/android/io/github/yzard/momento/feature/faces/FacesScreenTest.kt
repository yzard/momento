package io.github.yzard.momento.feature.faces

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FacesScreenTest {
    @Test
    fun mergeRequiresTwoGroupsAndNoActiveRequest() {
        assertFalse(canMergeFaceGroups(emptySet(), working = false))
        assertFalse(canMergeFaceGroups(setOf(1), working = false))
        assertTrue(canMergeFaceGroups(setOf(1, 2), working = false))
        assertFalse(canMergeFaceGroups(setOf(1, 2), working = true))
    }
}
