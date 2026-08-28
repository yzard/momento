package io.github.yzard.momento.core.data

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.SerializationException
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import retrofit2.HttpException
import retrofit2.Response
import java.io.IOException

class RequestResultTest {
    @Test
    fun successfulRequestPreservesResponse() = runBlocking {
        assertEquals(RequestResult.Success("loaded"), runRequest { "loaded" })
    }

    @Test
    fun networkFailureIsClassified() = runBlocking {
        assertEquals(RequestResult.Failure(RequestError.Network), runRequest<String> { throw IOException("offline") })
    }

    @Test
    fun httpFailurePreservesStatusCode() = runBlocking {
        val body = "unavailable".toResponseBody("text/plain".toMediaType())
        val failure = runRequest<String> { throw HttpException(Response.error<String>(503, body)) }

        assertEquals(RequestResult.Failure(RequestError.HTTP(503)), failure)
    }

    @Test
    fun invalidResponseIsClassified() = runBlocking {
        val failure = runRequest<String> { throw SerializationException("bad payload") }

        assertTrue(failure is RequestResult.Failure)
        assertEquals(RequestError.InvalidResponse, (failure as RequestResult.Failure).error)
    }

    @Test
    fun userMessageRetainsUsefulFailureContext() {
        assertEquals(
            "Could not load media. The server returned HTTP 403.",
            RequestError.HTTP(403).userMessage("Could not load media"),
        )
    }
}
