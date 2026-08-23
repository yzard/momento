package io.github.yzard.momento.feature.media

import io.github.yzard.momento.core.model.Media
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class MediaPagingStateTest {
    @Test fun appendPageRemovesRepeatedMediaAcrossCursors() {
        val first = media(1)
        val state = MediaPagingState(listOf(first), "cursor-1", true, true, null)
        val result = appendMediaPage(state, listOf(first, media(2)), null, false)
        assertEquals(listOf(1L, 2L), result.items.map { it.id })
        assertFalse(result.hasMore)
    }

    private fun media(id: Long) = Media(id, "$id.jpg", "$id.jpg", "image", createdAt = "2026-01-01T00:00:00Z")
}
