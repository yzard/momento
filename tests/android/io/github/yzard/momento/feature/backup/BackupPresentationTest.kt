package io.github.yzard.momento.feature.backup

import org.junit.Assert.assertEquals
import org.junit.Test

class BackupPresentationTest {
    @Test fun sizesUseMBOrGBWithAtMostTwoDecimalPlaces() {
        for ((bytes, expected) in listOf(-1L to "0 MB", 0L to "0 MB", 999L to "0 MB",
            12340L to "0.01 MB", 1000000L to "1 MB", 2675000L to "2.68 MB",
            12345678L to "12.35 MB", 1000000000L to "1 GB", 2345678901L to "2.35 GB")) {
            assertEquals(expected, backupByteCount(bytes))
        }
        assertEquals("9223372036.85 GB", backupByteCount(Long.MAX_VALUE))
        assertEquals("Backing up Original MOV.mov 12.34 MB/2 GB", backupFileStatus("Original MOV.mov", 12340000, 2000000000))
        assertEquals("Original.MOV: Upload failed — Cannot Open", backupUploadFailure("Original.MOV", "Cannot Open"))
    }    @Test fun failureUsesTheServerReasonAndKeepsTheOriginalFilename() {
        assertEquals("Original.MOV: Upload failed — Original exceeds upload limit",
            backupUploadFailure("Original.MOV", "retrofit2.HttpException: HTTP 413\nServer response: {\"detail\":\"Original exceeds upload limit\"}"))
        assertEquals("Network unavailable", backupUploadReason("java.io.IOException: Network unavailable\n at worker"))
        assertEquals("Unknown upload error", backupUploadReason(""))
    }

}
