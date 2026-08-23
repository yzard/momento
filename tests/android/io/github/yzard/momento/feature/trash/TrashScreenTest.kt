package io.github.yzard.momento.feature.trash
import org.junit.Assert.assertEquals
import org.junit.Test
class TrashScreenTest { @Test fun selectionIsSetBased() { assertEquals(setOf(2L), emptySet<Long>() + 2L) } }
