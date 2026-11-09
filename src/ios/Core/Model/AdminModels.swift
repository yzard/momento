import Foundation

struct DeduplicateStatusResponse: Codable, Sendable {
    var status: String
    var runId: Int64?
    var trigger: String?
    var scheduledFor: String?
    var startedAt: String?
    var completedAt: String?
    var ensembledMedia: Int64
    var processedMedia: Int64
    var candidateComparisons: Int64
    var clustersCreated: Int64
    var error: String?
    var jobs: AiJobCounts
}

struct AdminUserCreateRequest: Codable, Sendable {
    var username: String
    var email: String
    var password: String
    var role: String?
}

struct AdminUserUpdateRequest: Codable, Sendable {
    var userId: Int64
    var role: String?
    var isActive: Bool?
}

struct AdminUserIdRequest: Codable, Sendable {
    var userId: Int64
}

struct MetadataActionResponse: Codable, Sendable {
    var message: String
    var affectedJobs: Int64
}

struct MetadataStatus: Codable, Sendable {
    var status: String
    var queuedJobs: Int64
    var processingJobs: Int64
    var cancellingJobs: Int64
    var completedJobs: Int64
    var failedJobs: Int64
    var errors: [String]
    var waitingForRollbackJobs: Int64
}

struct AiFeatureActionResult: Codable, Sendable {
    var feature: String
    var outcome: String
    var affectedJobs: Int64
    var error: String?
}

struct AiActionResponse: Codable, Sendable {
    var action: String
    var results: [AiFeatureActionResult]
}

struct AiJobCounts: Codable, Sendable {
    var queued: Int64
    var submitting: Int64
    var submitted: Int64
    var completed: Int64
    var failed: Int64
    var cancelled: Int64
}

struct AiTaskStatus: Codable, Sendable {
    var task: String
    var enabled: Bool
    var state: String
    var jobs: AiJobCounts
    var errors: [String]
}

struct AiFeatureSchedule: Codable, Sendable {
    var feature: String
    var cronExpression: String
}

struct AiScheduleUpdateRequest: Codable, Sendable {
    var feature: String
    var cronExpression: String
}

struct AiStatusResponse: Codable, Sendable {
    var tasks: [AiTaskStatus]
    var deduplicate: DeduplicateStatusResponse
    var faceGroups: Int64
    var schedules: [AiFeatureSchedule]
}

struct ImportStatus: Codable, Sendable {
    var status: String
    var totalFiles: Int64
    var processedFiles: Int64
    var totalMedia: Int64
    var successfulImports: Int64
    var failedImports: Int64
    var startedAt: String?
    var completedAt: String?
    var errors: [String]
}
