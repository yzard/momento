package io.github.yzard.momento.core.model

import org.junit.Assert.assertEquals
import org.junit.Test

class ModelsTest {
    @Test fun backupStatesSeparateRetryableAndTerminalFailures() { assertEquals(listOf("QUEUED", "UPLOADING", "COMPLETING", "SERVER_PROCESSING", "COMPLETED", "FAILED", "TERMINAL_FAILED", "CANCELLING", "CANCELLED"), BackupState.entries.map { it.name }) }
}
