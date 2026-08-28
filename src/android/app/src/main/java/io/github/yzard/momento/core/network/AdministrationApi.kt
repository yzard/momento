package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.*
import retrofit2.http.Body
import retrofit2.http.POST
import retrofit2.http.Path

interface AdministrationApi {
    @POST("api/v1/user/list") suspend fun users(): UsersResponse
    @POST("api/v1/user/create") suspend fun createUser(@Body request: AdminUserCreateRequest): User
    @POST("api/v1/user/update") suspend fun updateUser(@Body request: AdminUserUpdateRequest): User
    @POST("api/v1/user/delete") suspend fun deleteUser(@Body request: AdminUserIdRequest): MessageResponse
    @POST("api/v1/import/local") suspend fun triggerLocalImport(): MessageResponse
    @POST("api/v1/import/status") suspend fun importStatus(): ImportStatus
    @POST("api/v1/metadata/generate") suspend fun generateMetadata(@Body request: EmptyRequest): JobActionResponse
    @POST("api/v1/metadata/status") suspend fun metadataStatus(@Body request: EmptyRequest): JobStatus
    @POST("api/v1/metadata/reset") suspend fun resetMetadata(@Body request: EmptyRequest): JobActionResponse
    @POST("api/v1/ai/start") suspend fun startAi(): AiActionResponse
    @POST("api/v1/ai/status") suspend fun aiStatus(): AiStatusResponse
    @POST("api/v1/ai/cancel") suspend fun cancelAi(): AiActionResponse
    @POST("api/v1/ai/clean") suspend fun cleanAi(): AiActionResponse
    @POST("api/v1/ai/schedule/update") suspend fun updateAiSchedule(@Body request: AiScheduleUpdateRequest): AiFeatureSchedule
    @POST("api/v1/ai/{feature}/start") suspend fun startAiFeature(@Path("feature") feature: String): AiActionResponse
    @POST("api/v1/ai/{feature}/cancel") suspend fun cancelAiFeature(@Path("feature") feature: String): AiActionResponse
    @POST("api/v1/ai/{feature}/clean") suspend fun cleanAiFeature(@Path("feature") feature: String): AiActionResponse
}
