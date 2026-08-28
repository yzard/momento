package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PageStateTest {
    @Test
    fun `refresh preserves visible content`() {
        val state = PageState.Ready(listOf(1L, 2L), refreshing = false)

        assertEquals(
            PageState.Ready(listOf(1L, 2L), refreshing = true),
            state.beginRefresh(),
        )
    }

    @Test
    fun `initial failure has an explicit failed state`() {
        val state = PageState.Loading.failRefresh<String>("Offline")

        assertTrue(state is PageState.Failed)
        assertEquals("Offline", (state as PageState.Failed).message)
    }

    @Test
    fun `refresh failure keeps previously rendered content`() {
        val state = PageState.Ready("content", refreshing = true)

        assertEquals(
            PageState.Ready("content", refreshing = false),
            state.failRefresh("Offline"),
        )
    }
}
