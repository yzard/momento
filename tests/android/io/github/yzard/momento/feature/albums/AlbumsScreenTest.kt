package io.github.yzard.momento.feature.albums
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlinx.coroutines.runBlocking
import java.io.IOException

class AlbumsScreenTest {
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
}
