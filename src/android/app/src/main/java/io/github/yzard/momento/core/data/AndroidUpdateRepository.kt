package io.github.yzard.momento.core.data

import java.io.File

interface AndroidUpdateRepository {
    suspend fun downloadAndroidApk(destination: File)
}
