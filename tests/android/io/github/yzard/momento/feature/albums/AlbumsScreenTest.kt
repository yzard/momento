package io.github.yzard.momento.feature.albums
import org.junit.Assert.assertEquals
import org.junit.Test
class AlbumsScreenTest { @Test fun movesSelectedAlbumMediaEarlier() { assertEquals(listOf(2L, 1L, 3L), reorderAlbumIds(listOf(1, 2, 3), 2, -1)) } }
