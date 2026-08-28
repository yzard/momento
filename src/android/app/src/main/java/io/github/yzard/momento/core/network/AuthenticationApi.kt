package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.Capabilities
import io.github.yzard.momento.core.model.ChangePasswordRequest
import io.github.yzard.momento.core.model.LogoutRequest
import io.github.yzard.momento.core.model.MessageResponse
import io.github.yzard.momento.core.model.RefreshTokenRequest
import io.github.yzard.momento.core.model.TokenPair
import io.github.yzard.momento.core.model.User
import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.Header
import retrofit2.http.POST

interface AuthenticationApi {
    @GET("api/v1/client/capabilities") suspend fun capabilities(): Capabilities
    @POST("api/v1/user/authenticate") suspend fun login(@Header("Authorization") basic: String): TokenPair
    @POST("api/v1/user/refresh") suspend fun refresh(@Body request: RefreshTokenRequest): TokenPair
    @POST("api/v1/user/get") suspend fun currentUser(): User
    @POST("api/v1/user/logout") suspend fun logout(@Body request: LogoutRequest): MessageResponse
    @POST("api/v1/user/change-password") suspend fun changePassword(@Body request: ChangePasswordRequest): MessageResponse
}
