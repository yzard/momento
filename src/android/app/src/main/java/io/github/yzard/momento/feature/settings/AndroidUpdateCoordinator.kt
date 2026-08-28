package io.github.yzard.momento.feature.settings

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.FileProvider
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.AndroidUpdateRepository
import java.io.File
import java.io.IOException

enum class AndroidUpdateDecision { INVALID_PACKAGE, UP_TO_DATE, UPDATE_AVAILABLE }

data class AndroidUpdateIdentity(val versionCode: Long, val buildTimeMillis: Long)

sealed interface AndroidUpdateCheckResult {
    data class Finished(val message: String) : AndroidUpdateCheckResult
    data class Available(val file: File, val message: String) : AndroidUpdateCheckResult
}

private const val ANDROID_BUILD_TIME_METADATA_KEY = "io.github.yzard.momento.BUILD_TIME"
private const val ANDROID_BUILD_TIME_PREFIX = "epochMillis:"
private const val UPDATE_DIRECTORY_NAME = "app_updates"
private const val UPDATE_DOWNLOAD_FILENAME = "momento-update.download"

fun androidUpdateDecision(
    installedVersionCode: Long,
    installedBuildTimeMillis: Long,
    candidateVersionCode: Long,
    candidateBuildTimeMillis: Long?,
    packageMatches: Boolean,
): AndroidUpdateDecision = when {
    !packageMatches || candidateBuildTimeMillis == null -> AndroidUpdateDecision.INVALID_PACKAGE
    candidateVersionCode < installedVersionCode -> AndroidUpdateDecision.UP_TO_DATE
    candidateVersionCode > installedVersionCode -> AndroidUpdateDecision.UPDATE_AVAILABLE
    candidateBuildTimeMillis > installedBuildTimeMillis -> AndroidUpdateDecision.UPDATE_AVAILABLE
    else -> AndroidUpdateDecision.UP_TO_DATE
}

fun androidBuildTimeMillis(metadataValue: String?): Long? {
    if (metadataValue?.startsWith(ANDROID_BUILD_TIME_PREFIX) != true) return null
    return metadataValue.removePrefix(ANDROID_BUILD_TIME_PREFIX).toLongOrNull()?.takeIf { it > 0 }
}

fun androidUpdateCacheFilename(identity: AndroidUpdateIdentity): String =
    "momento-update-${identity.versionCode}-${identity.buildTimeMillis}.apk"

fun cachedAndroidUpdateIdentity(filename: String): AndroidUpdateIdentity? {
    val match = Regex("^momento-update-([0-9]+)-([0-9]+)\\.apk$").matchEntire(filename) ?: return null
    val versionCode = match.groupValues[1].toLongOrNull() ?: return null
    val buildTimeMillis = match.groupValues[2].toLongOrNull() ?: return null
    return AndroidUpdateIdentity(versionCode, buildTimeMillis)
}

class AndroidUpdateCoordinator(
    private val context: Context,
    private val repository: AndroidUpdateRepository,
) {
    suspend fun check(): AndroidUpdateCheckResult {
        val updateDirectory = updateDirectory()
        clearDirectory(updateDirectory)
        if (!updateDirectory.exists() && !updateDirectory.mkdirs()) {
            return AndroidUpdateCheckResult.Finished("Could not prepare Android update storage.")
        }
        val downloadFile = File(updateDirectory, UPDATE_DOWNLOAD_FILENAME)
        return try {
            repository.downloadAndroidApk(downloadFile)
            evaluateDownloadedPackage(downloadFile)
        } catch (_: IOException) {
            downloadFile.delete()
            AndroidUpdateCheckResult.Finished("Could not download an Android update from this host.")
        }
    }

    fun installerIntent(updateFile: File): Intent {
        val uri = FileProvider.getUriForFile(
            context,
            "${context.packageName}.fileprovider",
            updateFile,
        )
        return Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
    }

    fun clearObsoleteUpdates() {
        val updateDirectory = updateDirectory()
        File(updateDirectory, UPDATE_DOWNLOAD_FILENAME).delete()
        updateDirectory.listFiles()?.forEach { file ->
            val identity = cachedAndroidUpdateIdentity(file.name) ?: return@forEach
            val decision = androidUpdateDecision(
                installedVersionCode = BuildConfig.VERSION_CODE.toLong(),
                installedBuildTimeMillis = BuildConfig.BUILD_TIME_MILLIS,
                candidateVersionCode = identity.versionCode,
                candidateBuildTimeMillis = identity.buildTimeMillis,
                packageMatches = true,
            )
            if (decision != AndroidUpdateDecision.UPDATE_AVAILABLE) file.delete()
        }
    }

    private fun evaluateDownloadedPackage(downloadFile: File): AndroidUpdateCheckResult {
        val packageInfo = context.packageManager.readArchivePackageInfo(downloadFile)
        if (packageInfo == null) {
            downloadFile.delete()
            return AndroidUpdateCheckResult.Finished("The host did not provide a valid Momento APK.")
        }
        val candidateVersionCode = packageInfo.compatibleLongVersionCode()
        val candidateBuildTimeMillis = packageInfo.androidBuildTimeMillis()
        return when (
            androidUpdateDecision(
                installedVersionCode = BuildConfig.VERSION_CODE.toLong(),
                installedBuildTimeMillis = BuildConfig.BUILD_TIME_MILLIS,
                candidateVersionCode = candidateVersionCode,
                candidateBuildTimeMillis = candidateBuildTimeMillis,
                packageMatches = packageInfo.packageName == context.packageName,
            )
        ) {
            AndroidUpdateDecision.INVALID_PACKAGE -> {
                downloadFile.delete()
                AndroidUpdateCheckResult.Finished("The host APK is not a Momento Android package.")
            }
            AndroidUpdateDecision.UP_TO_DATE -> {
                downloadFile.delete()
                AndroidUpdateCheckResult.Finished("Momento ${BuildConfig.VERSION_NAME} is up to date.")
            }
            AndroidUpdateDecision.UPDATE_AVAILABLE -> stageUpdate(
                downloadFile,
                packageInfo,
                candidateVersionCode,
                requireNotNull(candidateBuildTimeMillis),
            )
        }
    }

    private fun stageUpdate(
        downloadFile: File,
        packageInfo: PackageInfo,
        candidateVersionCode: Long,
        candidateBuildTimeMillis: Long,
    ): AndroidUpdateCheckResult {
        val updateFile = File(
            requireNotNull(downloadFile.parentFile),
            androidUpdateCacheFilename(AndroidUpdateIdentity(candidateVersionCode, candidateBuildTimeMillis)),
        )
        if (!downloadFile.renameTo(updateFile)) throw IOException("Could not stage the Android update")
        val message = if (candidateVersionCode == BuildConfig.VERSION_CODE.toLong()) {
            "A newer build of Momento ${BuildConfig.VERSION_NAME} is ready to install."
        } else {
            "Version ${packageInfo.versionName ?: candidateVersionCode} is ready to install."
        }
        return AndroidUpdateCheckResult.Available(updateFile, message)
    }

    private fun updateDirectory(): File = File(context.cacheDir, UPDATE_DIRECTORY_NAME)

    private fun clearDirectory(directory: File) {
        directory.listFiles()?.forEach { it.delete() }
    }
}

private fun PackageManager.readArchivePackageInfo(file: File): PackageInfo? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getPackageArchiveInfo(
            file.absolutePath,
            PackageManager.PackageInfoFlags.of(PackageManager.GET_META_DATA.toLong()),
        )
    } else {
        @Suppress("DEPRECATION")
        getPackageArchiveInfo(file.absolutePath, PackageManager.GET_META_DATA)
    }

private fun PackageInfo.androidBuildTimeMillis(): Long? =
    androidBuildTimeMillis(applicationInfo?.metaData?.getString(ANDROID_BUILD_TIME_METADATA_KEY))

private fun PackageInfo.compatibleLongVersionCode(): Long =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) longVersionCode else {
        @Suppress("DEPRECATION")
        versionCode.toLong()
    }
