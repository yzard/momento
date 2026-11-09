package io.github.yzard.momento.feature.settings

import android.content.Intent
import android.net.Uri
import androidx.core.content.IntentCompat
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupDiagnosticsShareInstrumentedTest {
    @Test fun sharesCompleteRedactedLogsThroughAReadOnlyContentUri() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val report = "Bearer fixture-secret\n" + "Recent diagnostic event\n".repeat(10_000)
        val intent = backupDiagnosticsShareIntent(context, report)
        val uri = requireNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
        try {
            assertEquals(Intent.ACTION_SEND, intent.action)
            assertEquals("text/plain", intent.type)
            assertEquals("content", uri.scheme)
            assertTrue(intent.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
            assertEquals(0, intent.flags and Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            assertEquals(uri, intent.clipData?.getItemAt(0)?.uri)
            val text = context.contentResolver.openInputStream(uri)!!.bufferedReader().use { it.readText() }
            assertFalse(text.contains("fixture-secret"))
            assertEquals(10_000, text.lineSequence().count { it == "Recent diagnostic event" })
        } finally {
            context.contentResolver.delete(uri, null, null)
        }
    }
}
