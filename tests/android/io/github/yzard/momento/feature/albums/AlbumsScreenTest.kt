package io.github.yzard.momento.feature.albums
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlinx.coroutines.runBlocking
import java.io.IOException
import io.github.yzard.momento.core.model.AlbumDetail
import io.github.yzard.momento.core.model.Media

class AlbumsScreenTest {
    @Test fun choosesTheExpectedCollageForUpToFourThumbnails() {
        assertEquals(AlbumCollageLayout.EMPTY, albumCollageLayout(0))
        assertEquals(AlbumCollageLayout.SINGLE, albumCollageLayout(1))
        assertEquals(AlbumCollageLayout.TWO_COLUMNS, albumCollageLayout(2))
        assertEquals(AlbumCollageLayout.LARGE_LEFT, albumCollageLayout(3))
        assertEquals(AlbumCollageLayout.GRID, albumCollageLayout(4))
        assertEquals(AlbumCollageLayout.GRID, albumCollageLayout(8))
    }

    @Test fun formatsAlbumMemoryCounts() {
        assertEquals("0 memories", albumMemoryCountLabel(0))
        assertEquals("1 memory", albumMemoryCountLabel(1))
        assertEquals("4 memories", albumMemoryCountLabel(4))
    }

    @Test fun identifiesRemovalAsAnAlbumOnlyAction() {
        assertEquals("Remove from album (2)", removeFromAlbumSelectionLabel(2))
    }

    @Test fun combinesTheAlbumCountAndDescriptionInTheFloatingTitle() {
        val album = AlbumDetail(
            id = 1,
            name = "Summer",
            description = "At the lake",
            coverMediaId = null,
            media = listOf(media(1), media(2)),
            createdAt = "2026-01-01T00:00:00Z",
        )

        assertEquals("2 memories · At the lake", albumDetailSubtitle(album))
    }

    @Test fun movesSelectedAlbumMediaEarlier() {
        assertEquals(listOf(2L, 1L, 3L), reorderAlbumIds(listOf(1, 2, 3), 2, -1))
    }

    @Test fun missingOrOnlyAlbumMediaCannotBeReordered() {
        assertEquals(emptyList<Long>(), reorderAlbumIds(emptyList(), 1, 1))
        assertEquals(listOf(1L), reorderAlbumIds(listOf(1), 1, 1))
        assertEquals(listOf(1L, 2L), reorderAlbumIds(listOf(1, 2), 3, -1))
    }

    @Test fun albumOperationReportsExpectedNetworkFailures() = runBlocking {
        assertTrue(executeAlbumOperation {})
        assertFalse(executeAlbumOperation { throw IOException("offline") })
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "photo",
        createdAt = "2026-01-01T00:00:00Z",
    )
}
