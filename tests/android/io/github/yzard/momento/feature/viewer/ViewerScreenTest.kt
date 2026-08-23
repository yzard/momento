package io.github.yzard.momento.feature.viewer
import org.junit.Assert.assertEquals
import org.junit.Test
class ViewerScreenTest { @Test fun clampsNavigationAtBothEnds() { assertEquals(0, viewerIndex(0, -1, 2)); assertEquals(1, viewerIndex(1, 1, 2)) }; @Test fun removalClampsAtNewEnd() { assertEquals(0, removeViewedMedia(emptyList(), 0).second) } }
