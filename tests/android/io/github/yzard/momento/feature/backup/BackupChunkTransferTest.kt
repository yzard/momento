package io.github.yzard.momento.feature.backup

import io.github.yzard.momento.core.model.BackupUploadResponse
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.net.SocketTimeoutException

class BackupChunkTransferTest {
    private val start = 5L * 1024 * 1024 * 1024
    private val length = 1024 * 1024
    private val total = start + length
    private fun response(offset: Long) = BackupUploadResponse("upload", "uploading", offset, total, "hash", null, null)

    @Test fun lostAcknowledgementAfterFiveGiBContinuesWithoutResending() = runBlocking {
        var sends = 0
        val result = transferBackupChunk("upload", start, length, total, "hash",
            send = { sends++; throw SocketTimeoutException("timeout") },
            status = { response(total) }, pause = {}, onRetry = {})
        assertEquals(total, result.uploadedSize)
        assertEquals(1, sends)
    }

    @Test fun uncommittedChunkIsResentAfterStatusConfirmation() = runBlocking {
        var sends = 0
        var queries = 0
        val result = transferBackupChunk("upload", start, length, total, "hash",
            send = { if (++sends == 1) throw SocketTimeoutException("timeout") else response(total) },
            status = { queries++; response(start) }, pause = {}, onRetry = {})
        assertEquals(total, result.uploadedSize)
        assertEquals(2, sends)
        assertEquals(1, queries)
    }

    @Test fun repeatedTimeoutsStopAfterBoundedRetries() = runBlocking {
        var sends = 0
        val pauses = mutableListOf<Long>()
        try {
            transferBackupChunk("upload", start, length, total, "hash",
                send = { sends++; throw SocketTimeoutException("timeout") },
                status = { response(start) }, pause = { pauses.add(it) }, onRetry = {})
            error("Expected timeout")
        } catch (_: SocketTimeoutException) {
            assertEquals(4, sends)
            assertEquals(listOf(1000L, 2000L, 4000L), pauses)
        }
    }

    @Test fun unexpectedServerOffsetCannotSkipMediaBytes() = runBlocking {
        try {
            transferBackupChunk("upload", start, length, total, "hash",
                send = { throw SocketTimeoutException("timeout") },
                status = { response(start + 1) }, pause = {}, onRetry = {})
            error("Expected invalid acknowledgement")
        } catch (error: IOException) {
            assertTrue(error.message!!.contains("Unexpected backup acknowledgement"))
        }
    }

    @Test fun busyChunkRemainsRetryableAfterAttemptsAreExhausted() = runBlocking {
        var sends = 0
        try {
            transferBackupChunk("upload", start, length, total, "hash",
                send = {
                    sends++
                    throw retrofit2.HttpException(retrofit2.Response.error<BackupUploadResponse>(
                        409, okhttp3.ResponseBody.create(null, "busy")))
                },
                status = { response(start) }, pause = {}, onRetry = {})
            error("Expected retryable failure")
        } catch (error: IOException) {
            assertEquals(4, sends)
            assertTrue(error.cause is retrofit2.HttpException)
        }
    }

    @Test fun failedStatusQueryDoesNotBlindlyResend() = runBlocking {
        var sends = 0
        try {
            transferBackupChunk("upload", start, length, total, "hash",
                send = { sends++; throw SocketTimeoutException("upload timeout") },
                status = { throw SocketTimeoutException("status timeout") }, pause = {}, onRetry = {})
            error("Expected status failure")
        } catch (error: SocketTimeoutException) {
            assertEquals("status timeout", error.message)
            assertEquals(1, sends)
        }
    }

    @Test fun cancellationStopsWithoutStatusQueryOrRetry() = runBlocking {
        var queries = 0
        try {
            transferBackupChunk("upload", start, length, total, "hash",
                send = { throw kotlinx.coroutines.CancellationException("cancelled") },
                status = { queries++; response(start) }, pause = {}, onRetry = {})
            error("Expected cancellation")
        } catch (_: kotlinx.coroutines.CancellationException) {
            assertEquals(0, queries)
        }
    }
}
