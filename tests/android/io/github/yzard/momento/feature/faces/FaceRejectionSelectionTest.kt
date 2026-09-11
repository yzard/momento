package io.github.yzard.momento.feature.faces

import io.github.yzard.momento.core.model.FaceDetection
import org.junit.Assert.assertEquals
import org.junit.Test

class FaceRejectionSelectionTest {
    private fun face(id: Long) = FaceDetection(id, 10, 0, 0.1, 0.1, 0.2, 0.2)
    @Test fun selectingAmbiguousMediaRequiresExplicitFaceChoices() {
        assertEquals(setOf(99L), toggleMediaFaceSelection(setOf(99L), listOf(face(1),face(2)), true))
    }
    @Test fun unambiguousMediaSelectsOnlyItsFaceAndDeselectPreservesOtherMedia() {
        assertEquals(setOf(99L,1L), toggleMediaFaceSelection(setOf(99L), listOf(face(1)), true))
        assertEquals(setOf(99L), toggleMediaFaceSelection(setOf(99L,1L), listOf(face(1)), false))
    }
}
