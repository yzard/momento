package io.github.yzard.momento.core.network

import android.content.Context
import coil.ImageLoader
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.model.RefreshTokenRequest
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Response
import retrofit2.HttpException
import retrofit2.Retrofit
import retrofit2.converter.kotlinx.serialization.asConverterFactory
import java.util.Base64

@OptIn(ExperimentalSerializationApi::class)
class NetworkClient(private val tokenStore: EncryptedTokenStore) {
    private val json = Json { ignoreUnknownKeys = true; explicitNulls = false }
    private val refreshMutex = Mutex()
    private val client = OkHttpClient.Builder().addInterceptor(BearerInterceptor(tokenStore, this)).build()
    private var currentOrigin: String? = null
    private var currentApi: MomentoApi? = null
    private var imageLoader: ImageLoader? = null

    @Synchronized
    fun api(origin: String): MomentoApi {
        if (origin == currentOrigin && currentApi != null) return requireNotNull(currentApi)
        return createApi(origin, client).also { currentOrigin = origin; currentApi = it }
    }

    fun httpClient(): OkHttpClient = client

    @Synchronized
    fun imageLoader(context: Context): ImageLoader {
        if (imageLoader != null) return requireNotNull(imageLoader)
        return ImageLoader.Builder(context.applicationContext)
            .okHttpClient { client }
            .build()
            .also { imageLoader = it }
    }

    private fun createApi(origin: String, requestClient: OkHttpClient): MomentoApi = Retrofit.Builder()
        .baseUrl("${origin.removeSuffix("/")}/")
        .client(requestClient)
        .addConverterFactory(json.asConverterFactory("application/json".toMediaType()))
        .build()
        .create(MomentoApi::class.java)

    suspend fun refresh(origin: String, rejectedToken: String): Boolean = refreshMutex.withLock {
        val currentToken = tokenStore.accessToken()
        if (currentToken != null && currentToken != rejectedToken) return@withLock true
        val refreshToken = tokenStore.refreshToken()
        if (refreshToken == null) {
            tokenStore.clear()
            return@withLock false
        }
        try {
            tokenStore.saveTokens(createApi(origin, OkHttpClient()).refresh(RefreshTokenRequest(refreshToken)))
            true
        } catch (error: HttpException) {
            if (error.code() == 401 || error.code() == 403) tokenStore.clear()
            false
        }
    }
}

internal fun authorizationHeader(existing: String?, accessToken: String?): String? = existing ?: accessToken?.let { "Bearer $it" }

internal class BearerInterceptor(
    private val tokenStore: EncryptedTokenStore,
    private val networkClient: NetworkClient,
) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val original = chain.request()
        val existingAuthorization = original.header("Authorization")
        val token = tokenStore.accessToken()
        val authorization = authorizationHeader(existingAuthorization, token)
        val request = if (authorization == existingAuthorization) original else original.newBuilder().header("Authorization", requireNotNull(authorization)).build()
        val response = chain.proceed(request)
        if (existingAuthorization != null || response.code != 401 || request.header("X-Momento-Retry") != null || token == null) return response

        val origin = request.url.newBuilder().encodedPath("/").query(null).fragment(null).build().toString().removeSuffix("/")
        val refreshed = runBlocking { networkClient.refresh(origin, token) }
        if (!refreshed) return response

        response.close()
        return chain.proceed(request.newBuilder().header("Authorization", "Bearer ${tokenStore.accessToken()}").header("X-Momento-Retry", "1").build())
    }
}

fun basicAuthorization(username: String, password: String): String = "Basic " + Base64.getEncoder().encodeToString("$username:$password".toByteArray())
