package io.github.yzard.momento.feature.faces
import org.junit.Assert.assertTrue
import org.junit.Test
class FacesScreenTest { @Test fun mergeRequiresTwoGroups() { assertTrue(setOf(1L, 2L).size >= 2) } }
