package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable

@Serializable data class DeduplicateStatusResponse(
    val status: String, val runId: Long?, val trigger: String?, val scheduledFor: String?,
    val startedAt: String?, val completedAt: String?, val ensembledMedia: Long,
    val processedMedia: Long, val candidateComparisons: Long, val clustersCreated: Long,
    val error: String?, val jobs: AiJobCounts,
)
@Serializable data class AdminUserCreateRequest(val username: String, val email: String, val password: String, val role: String?)
@Serializable data class AdminUserUpdateRequest(val userId: Long, val role: String?, val isActive: Boolean?)
@Serializable data class AdminUserIdRequest(val userId: Long)
@Serializable data class JobActionResponse(val message: String, val queuedJobs: Long)
@Serializable class EmptyRequest
@Serializable data class JobStatus(val status: String, val queuedJobs: Long, val processingJobs: Long, val completedJobs: Long, val failedJobs: Long, val errors: List<String>)
@Serializable data class AiFeatureActionResult(val feature: String, val outcome: String, val affectedJobs: Long, val error: String?)
@Serializable data class AiActionResponse(val action: String, val results: List<AiFeatureActionResult>)
@Serializable data class AiJobCounts(val queued: Long, val submitting: Long, val submitted: Long, val completed: Long, val failed: Long, val cancelled: Long)
@Serializable data class AiTaskStatus(val task: String, val enabled: Boolean, val state: String, val jobs: AiJobCounts, val errors: List<String>)
@Serializable data class AiFeatureSchedule(val feature: String, val cronExpression: String)
@Serializable data class AiScheduleUpdateRequest(val feature: String, val cronExpression: String)
@Serializable data class AiStatusResponse(val tasks: List<AiTaskStatus>, val deduplicate: DeduplicateStatusResponse, val faceGroups: Long, val schedules: List<AiFeatureSchedule>)
@Serializable data class ImportStatus(val status: String, val totalFiles: Long, val processedFiles: Long, val totalMedia: Long, val successfulImports: Long, val failedImports: Long, val startedAt: String?, val completedAt: String?, val errors: List<String>)

