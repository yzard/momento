package io.github.yzard.momento.feature.backup

import android.content.Context
import android.os.Build
import androidx.work.WorkManager
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.database.BackupAssetDao
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import retrofit2.HttpException
import java.io.File
import java.io.IOException
import java.time.Instant

internal fun redactBackupDiagnostic(text: String): String = text
    .replace(Regex("(?i)Bearer\\s+[^\\s\"]+"), "Bearer [redacted]")
    .replace(Regex("(?i)([\"']?(?:access_token|refresh_token|password|api_key|authorization|cookie)[\"']?\\s*[:=]\\s*)[^\\r\\n,}]+"), "$1[redacted]")
    .replace(Regex("(https?://)[^/\\s@]+@"), "$1[redacted]@")
    .replace(Regex("(https?://[^\\s?]+)\\?[^\\s]+"), "$1?[redacted]")

internal fun backupFailureDetail(error: Throwable): String {
    val response = if (error is HttpException) {
        try {
            error.response()?.errorBody()?.source()?.peek()?.use { source ->
                val truncated = source.request(8193)
                source.readUtf8(minOf(source.buffer.size, 8192)) + if (truncated) "\n[Response truncated at 8192 bytes]" else ""
            }
        } catch (_: IOException) {
            "[Response body unavailable]"
        }
    } else null
    return redactBackupDiagnostic(buildString {
        appendLine(error.stackTraceToString())
        if (response != null) appendLine("Server response: $response")
    })
}

/** Bounded local history; diagnostics must never change backup success/retry behavior. */
internal object BackupDiagnosticLog {
    private val lock = Any()
    suspend fun append(context: Context, event: String) = withContext(Dispatchers.IO) {
        try {
            synchronized(lock) {
                val file = File(context.filesDir, "backup-diagnostics.log")
                val previous = if (file.exists()) file.readText() else ""
                file.writeText((previous + "${Instant.now()} ${redactBackupDiagnostic(event)}\n").takeLast(96_000))
            }
        } catch (_: IOException) {
            // A full disk must not prevent the worker from recording its queue state.
        }
    }
    fun read(context: Context): String = synchronized(lock) {
        val file = File(context.filesDir, "backup-diagnostics.log")
        if (file.exists()) file.readText() else "No diagnostic events recorded yet."
    }
}

internal suspend fun backupDiagnosticReport(
    context: Context,
    settings: Settings,
    assets: BackupAssetDao,
): String = withContext(Dispatchers.IO) {
    val records = assets.observeAll().first()
    val report = buildString {
        appendLine("Momento backup diagnostics — ${Instant.now()}")
        appendLine("App ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}); build ${BuildConfig.BUILD_TIME_MILLIS}")
        appendLine("Device ${Build.MANUFACTURER} ${Build.MODEL}; Android ${Build.VERSION.RELEASE}; SDK ${Build.VERSION.SDK_INT}")
        appendLine("Camera only=${settings.cameraOnly}; mobile data=${settings.mobileDataEnabled}")
        appendLine("Network allowed=${isBackupNetworkAllowed(context, settings.mobileDataEnabled)}")
        appendLine("Media access=${currentBackupMediaAccess(context)}; location access=${currentBackupLocationMetadataAccess(context)}")
        appendLine("Queue: ${records.groupingBy { it.state }.eachCount()}")
        val manager = WorkManager.getInstance(context)
        for (name in listOf(IMMEDIATE_BACKUP_WORK_NAME, PERIODIC_BACKUP_WORK_NAME, BACKUP_CANCELLATION_WORK)) {
            for (work in manager.getWorkInfosForUniqueWorkFlow(name).first()) {
                appendLine("Work $name: ${work.id}, state=${work.state}, attempts=${work.runAttemptCount}, next=${work.nextScheduleTimeMillis}, stopReason=${work.stopReason}")
            }
        }
        appendLine("\nRecent local events (rolling history, up to 96,000 characters):")
        appendLine(BackupDiagnosticLog.read(context))
        appendLine("\nAll current records with errors (file names and correlation IDs included):")
        for (asset in records.filter { !it.errorMessage.isNullOrBlank() }) {
            appendLine("File=${asset.displayName}; state=${asset.state}; bytes=${asset.uploadedBytes}/${asset.byteSize}; protocol=${asset.protocolVersion}")
            appendLine("operation=${asset.operationId}; asset=${asset.clientAssetId}; upload=${asset.uploadId}; media=${asset.mediaId}")
            appendLine(asset.errorMessage)
        }
    }
    redactBackupDiagnostic(report)
}

internal fun conciseBackupIssue(detail: String): String =
    detail.lineSequence().firstOrNull { it.isNotBlank() }?.take(120) ?: "Backup could not finish. Copy diagnostic logs for details."
