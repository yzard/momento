package io.github.yzard.momento.feature.faces

import io.github.yzard.momento.core.model.FaceGroup
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FacesScreenTest {
    @Test
    fun appendsUniqueFaceGroupPages() {
        val first = FaceGroup(1, 2, 2)
        val second = FaceGroup(2, 3, 3)

        assertEquals(
            listOf(first, second),
            appendFaceGroups(listOf(first), listOf(first, second)),
        )
    }

    @Test
    fun mergeRequiresTwoGroupsAndNoActiveRequest() {
        assertFalse(canMergeFaceGroups(emptySet(), working = false))
        assertFalse(canMergeFaceGroups(setOf(1), working = false))
        assertTrue(canMergeFaceGroups(setOf(1, 2), working = false))
        assertFalse(canMergeFaceGroups(setOf(1, 2), working = true))
    }
}
