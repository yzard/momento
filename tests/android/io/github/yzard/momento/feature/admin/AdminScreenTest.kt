package io.github.yzard.momento.feature.admin

import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiTaskStatus
import io.github.yzard.momento.core.model.AiFeatureSchedule
import io.github.yzard.momento.core.model.AiStatusResponse
import io.github.yzard.momento.core.model.DeduplicateStatusResponse
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

        assertTrue(isActiveAiState(status.state))
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
    fun splitsAndRejoinsExactlyFiveCronFields() {
        val fields = listOf("15", "1", "*", "*", "1-5")

        assertEquals(fields, splitCronExpression(" 15  1 * * 1-5 "))
        assertEquals("15 1 * * 1-5", joinCronFields(fields))
        assertTrue(validCronFields(fields))
        assertFalse(validCronFields(listOf("15 30", "1", "*", "*", "1-5")))
        assertEquals(List(5) { "" }, splitCronExpression("15 1 * *"))
    }

    @Test fun adminSectionsMatchTheSharedClientOrder() {
        assertEquals(
            listOf("Import", "Metadata", "AI", "User Management"),
            AdminSection.entries.map { it.label },
        )
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsJoiningTheWrongCronFieldCount() {
        joinCronFields(listOf("15", "1", "*", "*"))
    }

    @Test
    fun selectsJobCountsForTaskAndDeduplicationRows() {
        val taskJobs = AiJobCounts(1, 2, 3, 4, 5, 6)
        val deduplicateJobs = AiJobCounts(7, 8, 9, 10, 11, 12)
        val status = AiStatusResponse(
            tasks = listOf(AiTaskStatus("ocr", true, "queued", taskJobs, emptyList())),
            deduplicate = DeduplicateStatusResponse(
                "running", null, null, null, null, null, 0, 0, 0, 0, null, deduplicateJobs,
            ),
            faceGroups = 0,
            schedules = listOf(AiFeatureSchedule("ocr", "0 2 * * *")),
        )

        assertEquals(taskJobs, aiJobCounts(status, AdminAiFeature.OCR))
        assertEquals(deduplicateJobs, aiJobCounts(status, AdminAiFeature.DEDUPLICATE))
    }

    @Test
    fun collectsTaskAndDeduplicationFailuresInStableOrder() {
        val emptyJobs = AiJobCounts(0, 0, 0, 0, 0, 0)
        val status = AiStatusResponse(
            tasks = listOf(
                AiTaskStatus("image_tagging", true, "failed", emptyJobs, listOf("tagging failed")),
                AiTaskStatus("ocr", true, "failed", emptyJobs, listOf("OCR failed")),
            ),
            deduplicate = DeduplicateStatusResponse(
                "failed", null, null, null, null, null, 0, 0, 0, 0, "deduplication failed", emptyJobs,
            ),
            faceGroups = 0,
            schedules = emptyList(),
        )

        assertEquals(
            listOf(
                "[OCR] OCR failed",
                "[Image tagging] tagging failed",
                "[Deduplication] deduplication failed",
            ),
            aiFailureLogEntries(status),
        )
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
