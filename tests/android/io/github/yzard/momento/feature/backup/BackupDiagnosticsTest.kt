package io.github.yzard.momento.feature.backup

import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import retrofit2.HttpException
import retrofit2.Response
import java.io.IOException

class BackupDiagnosticsTest {
    @Test fun capturesHttpBodyWithoutConsumingIt() {
        val body = "{\"detail\":\"Not enough storage\"}".toResponseBody()
        val error = HttpException(Response.error<Unit>(507, body))
        val detail = backupFailureDetail(error)
        assertTrue(detail.contains("507"))
        assertTrue(detail.contains("Not enough storage"))
        assertEquals(detail, backupFailureDetail(error))
        assertEquals("{\"detail\":\"Not enough storage\"}", body.string())
    }

    @Test fun includesNestedCauseButKeepsUiShort() {
        val detail = backupFailureDetail(IOException("Upload failed", IOException("Disk is full")))
        assertTrue(detail.contains("Disk is full"))
        assertEquals("java.io.IOException: Upload failed", conciseBackupIssue(detail))
        assertTrue(conciseBackupIssue("x".repeat(1000)).length <= 120)
    }

    @Test fun removesCredentialsFromErrors() {
        val redacted = redactBackupDiagnostic("Authorization: Bearer SECRET\n{\"refresh_token\":\"PRIVATE\"}\nhttps://user:PASS@example.com/api?token=QUERY")
        for (secret in listOf("SECRET", "PRIVATE", "PASS", "QUERY")) assertFalse(redacted.contains(secret))
        assertTrue(redacted.contains("example.com"))
    }

    @Test fun boundsServerErrorBody() {
        val error = HttpException(Response.error<Unit>(500, "x".repeat(20000).toResponseBody()))
        assertFalse(backupFailureDetail(error).contains("x".repeat(8193)))
    }
}
