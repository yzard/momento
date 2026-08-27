package io.github.yzard.momento.feature.trash

import io.github.yzard.momento.core.model.TrashMedia
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.Instant

class TrashScreenTest {
    @Test fun showsTheThirtyDayRetentionCountdown() {
        val now = Instant.parse("2026-08-27T12:00:00Z")
        assertEquals(30, trashDaysRemaining("2026-08-27T11:59:00Z", now))
        assertEquals(1, trashDaysRemaining("2026-07-29T12:00:00Z", now))
        assertEquals(0, trashDaysRemaining("2026-07-01T12:00:00Z", now))
        assertEquals("30-day retention", trashRetentionLabel("invalid", now))
    }

    @Test
    fun mapsTrashMediaIntoTheSharedGridModel() {
        val media = TrashMedia(
            id = 4,
            filename = "4.jpg",
            originalFilename = "photo.jpg",
            mediaType = "image",
            mimeType = "image/jpeg",
            width = 1200,
            height = 800,
            fileSize = 42,
            durationSeconds = null,
            dateTaken = "2026-08-23T12:30:00",
            deletedAt = "2026-08-24T12:30:00Z",
            createdAt = "2026-08-23T12:30:00Z",
        ).asMedia()

        assertEquals(4, media.id)
        assertEquals(1200, media.width)
        assertEquals("2026-08-23T12:30:00", media.dateTaken)
        assertNull(media.gpsLatitude)
    }
}
