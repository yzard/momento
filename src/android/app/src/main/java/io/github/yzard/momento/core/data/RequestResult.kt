package io.github.yzard.momento.core.data

import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

sealed interface RequestResult<out Response> {
    data class Success<Response>(val response: Response) : RequestResult<Response>
    data class Failure(val error: RequestError) : RequestResult<Nothing>
}

sealed interface RequestError {
    data object Network : RequestError
    data class HTTP(val statusCode: Int) : RequestError
    data object InvalidResponse : RequestError
}

suspend fun <Response> runRequest(request: suspend () -> Response): RequestResult<Response> = try {
    RequestResult.Success(request())
} catch (_: IOException) {
    RequestResult.Failure(RequestError.Network)
} catch (error: HttpException) {
    RequestResult.Failure(RequestError.HTTP(error.code()))
} catch (_: SerializationException) {
    RequestResult.Failure(RequestError.InvalidResponse)
}

fun RequestError.userMessage(action: String): String = when (this) {
    RequestError.Network -> "$action. Check the connection and try again."
    is RequestError.HTTP -> "$action. The server returned HTTP $statusCode."
    RequestError.InvalidResponse -> "$action. The server response was invalid."
}
