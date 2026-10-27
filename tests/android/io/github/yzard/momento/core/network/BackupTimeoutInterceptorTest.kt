package io.github.yzard.momento.core.network

import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Test

class BackupTimeoutInterceptorTest {
    @Test fun backupTimeoutsDoNotChangeOrdinaryRequests() {
        val observed = mutableListOf<List<Int>>()
        val client = OkHttpClient.Builder()
            .addInterceptor(BackupTimeoutInterceptor())
            .addInterceptor { chain ->
                observed.add(listOf(chain.connectTimeoutMillis(), chain.readTimeoutMillis(), chain.writeTimeoutMillis()))
                Response.Builder().request(chain.request()).protocol(Protocol.HTTP_1_1)
                    .code(200).message("OK").body("{}".toResponseBody()).build()
            }.build()
        for (path in listOf("backup/upload/chunk/upload", "backup/upload/complete", "media/list")) {
            client.newCall(Request.Builder().url("https://example.invalid/api/v1/$path").build()).execute().close()
        }
        assertEquals(listOf(30_000, 120_000, 120_000), observed[0])
        assertEquals(observed[0], observed[1])
        assertEquals(listOf(10_000, 10_000, 10_000), observed[2])
        assertEquals(0, client.callTimeoutMillis)
    }
}
