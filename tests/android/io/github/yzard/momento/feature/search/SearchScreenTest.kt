package io.github.yzard.momento.feature.search
import org.junit.Assert.assertEquals
import org.junit.Test
class SearchScreenTest { @Test fun normalizesWhitespaceBeforeDebounce() { assertEquals("cat", normalizedSearchQuery(" cat ")) } }
