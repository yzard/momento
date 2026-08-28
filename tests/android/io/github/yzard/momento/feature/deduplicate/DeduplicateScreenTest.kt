package io.github.yzard.momento.feature.deduplicate

import io.github.yzard.momento.core.model.DeduplicateGroup
import io.github.yzard.momento.core.model.Media
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeduplicateScreenTest {
    @Test fun comparisonCardsAndActionsAdaptAcrossPhoneAndTabletWidths() {
        assertEquals(2, deduplicateColumns(360))
        assertEquals(3, deduplicateColumns(600))
        assertEquals(4, deduplicateColumns(840))
        assertTrue(compactDeduplicateActions(360))
        assertFalse(compactDeduplicateActions(460))
    }
    @Test
    fun activeStatesContinuePolling() {
        assertTrue(activeDeduplicationState("queued"))
        assertTrue(activeDeduplicationState("running"))
        assertTrue(activeDeduplicationState("cancelling"))
        assertFalse(activeDeduplicationState("completed"))
    }

    @Test
    fun controlsRequireAdministrator() {
        assertTrue(canManageDeduplication(true))
        assertFalse(canManageDeduplication(false))
    }

    @Test
    fun rejectsSelectionsThatWouldRemoveAnEntireGroup() {
        val groups = listOf(
            DeduplicateGroup(10, listOf(media(1), media(2))),
            DeduplicateGroup(20, listOf(media(3), media(4), media(5))),
        )

        assertEquals(listOf(10L), groupsWithoutKeptMedia(groups, setOf(1, 2, 3)))
        assertTrue(groupsWithoutKeptMedia(groups, setOf(1, 3)).isEmpty())
    }

    @Test
    fun formatsExactTrashCountAndMediaDetails() {
        assertEquals("Move 1 selected item to Trash?", deduplicateTrashConfirmation(1))
        assertEquals("Move 3 selected items to Trash?", deduplicateTrashConfirmation(3))
        assertEquals("2.0 MiB", deduplicateFileSize(2L * 1024 * 1024))
        assertEquals("1920 × 1080", deduplicateDimensions(media(1).copy(width = 1920, height = 1080)))
        assertEquals("Unknown dimensions", deduplicateDimensions(media(1)))
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "image",
        createdAt = "2026-01-01T00:00:00Z",
    )
}
