package io.github.yzard.momento.core.model

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test

class ModelsTest {
    @Test fun backupStatesSeparateRetryableAndTerminalFailures() { assertEquals(listOf("QUEUED", "UPLOADING", "COMPLETING", "SERVER_PROCESSING", "COMPLETED", "FAILED", "TERMINAL_FAILED", "CANCELLING", "CANCELLED"), BackupState.entries.map { it.name }) }

    @Test fun aiActionUsesExactFeatureAndCamelCaseCounts() {
        val response = AiActionResponse(
            action = "start",
            results = listOf(AiFeatureActionResult("face_detection", "queued", 3, null)),
        )

        assertEquals(
            "{\"action\":\"start\",\"results\":[{\"feature\":\"face_detection\",\"outcome\":\"queued\",\"affectedJobs\":3,\"error\":null}]}",
            Json.encodeToString(response),
        )
    }
}
