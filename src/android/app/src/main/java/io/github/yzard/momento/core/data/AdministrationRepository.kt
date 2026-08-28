package io.github.yzard.momento.core.data

import io.github.yzard.momento.core.model.*

interface AdministrationRepository {
    suspend fun users(): List<User>
    suspend fun createUser(username: String, email: String, password: String, role: String?): User
    suspend fun updateUser(id: Long, role: String?, active: Boolean?): User
    suspend fun deleteUser(id: Long): MessageResponse
    suspend fun localImport(): MessageResponse
    suspend fun importStatus(): ImportStatus
    suspend fun generateMetadata(): JobActionResponse
    suspend fun metadataStatus(): JobStatus
    suspend fun resetMetadata(): JobActionResponse
    suspend fun startAi(): AiActionResponse
    suspend fun aiStatus(): AiStatusResponse
    suspend fun cancelAi(): AiActionResponse
    suspend fun cleanAi(): AiActionResponse
    suspend fun updateAiSchedule(feature: String, cronExpression: String): AiFeatureSchedule
    suspend fun startAiFeature(feature: String): AiActionResponse
    suspend fun cancelAiFeature(feature: String): AiActionResponse
    suspend fun cleanAiFeature(feature: String): AiActionResponse
}
