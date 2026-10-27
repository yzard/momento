package io.github.yzard.momento.core.network

import okhttp3.Interceptor
import okhttp3.Response
import java.util.concurrent.TimeUnit

/** Bound idle waits per request, without limiting the duration of an entire video backup. */
internal class BackupTimeoutInterceptor : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val backupChain = if (request.url.encodedPath.startsWith("/api/v1/backup/")) {
            chain.withConnectTimeout(30, TimeUnit.SECONDS)
                .withReadTimeout(120, TimeUnit.SECONDS)
                .withWriteTimeout(120, TimeUnit.SECONDS)
        } else chain
        return backupChain.proceed(request)
    }
}
