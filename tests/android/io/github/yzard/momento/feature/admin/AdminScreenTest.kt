package io.github.yzard.momento.feature.admin

import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiTaskStatus
import io.github.yzard.momento.core.model.ImportStatus
import io.github.yzard.momento.core.model.JobStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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

    @Test
    fun formatsLocalImportCountsIncludingTotalMedia() {
        val status = ImportStatus("completed", 5324, 5324, 5103, 5324, 0, null, null, emptyList())

        assertEquals(
            "completed: 5324/5324 processed, 5324 imported, 0 failed, 5103 total media",
            importStatusSummary(status),
        )
        assertEquals("Not loaded", importStatusSummary(null))
    }

    @Test
    fun formatsExactAiTransportStates() {
        val status = AiTaskStatus(
            task = "ocr",
            enabled = true,
            state = "submitting",
            jobs = AiJobCounts(
                queued = 3,
                submitting = 2,
                submitted = 1,
                completed = 9,
                failed = 1,
                cancelled = 4,
            ),
            errors = emptyList(),
        )

        assertEquals(
            "submitting: 3 queued, 2 submitting, 1 submitted, 9 completed, 1 failed",
            aiStatusSummary(status),
        )
    }

    @Test
    fun exposesEverySupportedAiControl() {
        assertEquals(
            listOf(
                "ocr",
                "image_tagging",
                "screenshot_detection",
                "document_detection",
                "image_aesthetics",
                "deduplicate",
                "face_detection",
            ),
            AdminAiFeature.entries.map { it.identifier },
        )
        assertTrue(isActiveAiState("queued"))
        assertTrue(isActiveAiState("submitted"))
        assertTrue(isActiveAiState("cancelling"))
        assertFalse(isActiveAiState("completed"))
    }

    @Test
    fun validatesNewUserCredentialsBeforeSubmission() {
        assertEquals("Username is required", newUserValidation("", "person@example.com", "longenough"))
        assertEquals("Email is required", newUserValidation("person", "", "longenough"))
        assertEquals("Password must be at least 8 characters", newUserValidation("person", "person@example.com", "short"))
        assertNull(newUserValidation("person", "person@example.com", "longenough"))
    }

    @Test
    fun formatsWebDavUrlFromConfiguredServer() {
        assertEquals("https://photos.example.com/webdav/", webDavUrl("https://photos.example.com/"))
        assertEquals("Server URL unavailable", webDavUrl(null))
    }
}
