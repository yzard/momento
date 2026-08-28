package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CursorPagingStateTest {
    private data class Entry(val id: Long)

    @Test
    fun resetClearsPreviousEntriesBeforeLoading() {
        val previous = CursorPagingState(
            entries = listOf(Entry(1)),
            nextCursor = "next",
            hasMore = true,
            loading = false,
            initialized = true,
            error = "old error",
        )

        assertEquals(emptyCursorPagingState<Entry>().copy(loading = true), beginCursorPage(previous, reset = true))
    }

    @Test
    fun appendRequiresAnAvailableCursor() {
        val exhausted = emptyCursorPagingState<Entry>().copy(initialized = true, hasMore = false)

        assertNull(beginCursorPage(exhausted, reset = false))
    }

    @Test
    fun completionDeduplicatesExistingAndRepeatedPageEntries() {
        val loading = emptyCursorPagingState<Entry>().copy(entries = listOf(Entry(1)), loading = true)

        val completed = completeCursorPage(
            state = loading,
            page = listOf(Entry(1), Entry(2), Entry(2), Entry(3)),
            nextCursor = "after",
            hasMore = true,
            key = Entry::id,
        )

        assertEquals(listOf(Entry(1), Entry(2), Entry(3)), completed.entries)
        assertEquals("after", completed.nextCursor)
        assertEquals(true, completed.initialized)
        assertEquals(false, completed.loading)
    }

    @Test
    fun failureKeepsExistingEntriesForInlineRetry() {
        val loading = emptyCursorPagingState<Entry>().copy(entries = listOf(Entry(1)), loading = true)

        assertEquals(
            loading.copy(loading = false, error = "Unavailable"),
            failCursorPage(loading, "Unavailable"),
        )
    }
}
