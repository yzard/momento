package io.github.yzard.momento.feature.admin

import io.github.yzard.momento.core.model.JobStatus
import org.junit.Assert.assertEquals
import org.junit.Test

class AdminScreenTest {
    @Test
    fun togglesBetweenAdministratorAndUserRoles() {
        assertEquals("user", toggledRole("admin"))
        assertEquals("admin", toggledRole("user"))
    }

    @Test
    fun formatsProcessingCounts() {
        val status = JobStatus("running", 3, 2, 9, 1, emptyList())

        assertEquals("running: 3 queued, 2 processing, 9 completed, 1 failed", statusSummary(status))
    }

    @Test
    fun formatsUnloadedStatus() {
        assertEquals("Not loaded", statusSummary(null))
    }
}
