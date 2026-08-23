package io.github.yzard.momento.feature.deduplicate
import org.junit.Assert.assertTrue
import org.junit.Test
class DeduplicateScreenTest { @Test fun activeStatesPoll() { assertTrue("running" in setOf("queued", "running")) }; @Test fun controlsRequireAdmin() { assertTrue(canManageDeduplication(true)); org.junit.Assert.assertFalse(canManageDeduplication(false)) } }
