package io.github.yzard.momento.feature.settings

import android.content.ClipData
import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
import io.github.yzard.momento.feature.backup.redactBackupDiagnostic
import java.io.File
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

internal suspend fun backupDiagnosticsShareIntent(context: Context, report: String): Intent = withContext(Dispatchers.IO) {
    val directory = File(context.cacheDir, "backup_diagnostics")
    if (!directory.isDirectory && !directory.mkdirs()) throw IOException("Could not create diagnostics directory")
    val file = File(directory, "momento-backup-diagnostics.txt")
    file.writeText(redactBackupDiagnostic(report))
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
    Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_STREAM, uri)
        clipData = ClipData.newRawUri("Backup diagnostics", uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
}
