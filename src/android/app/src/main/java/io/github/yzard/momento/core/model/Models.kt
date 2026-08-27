package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable data class TokenPair(val accessToken: String, val refreshToken: String, val tokenType: String)
@Serializable data class RefreshTokenRequest(val refreshToken: String)
@Serializable data class LogoutRequest(val refreshToken: String)
@Serializable data class ChangePasswordRequest(val currentPassword: String, val newPassword: String)
@Serializable data class MessageResponse(val message: String, val status: String? = null)
@Serializable data class User(val id: Long, val username: String, val email: String, val role: String, val mustChangePassword: Boolean, val isActive: Boolean, val createdAt: String)
@Serializable data class UsersResponse(val users: List<User>)

@Serializable data class Media(
    val id: Long, val filename: String, val originalFilename: String, val mediaType: String,
    val mimeType: String? = null, val width: Int? = null, val height: Int? = null,
    val fileSize: Long? = null, val durationSeconds: Double? = null, val dateTaken: String? = null,
    val gpsLatitude: Double? = null, val gpsLongitude: Double? = null, val cameraMake: String? = null,
    val cameraModel: String? = null, val lensMake: String? = null, val lensModel: String? = null,
    val iso: Int? = null, val exposureTime: String? = null, val fNumber: Double? = null,
    val focalLength: Double? = null, val focalLength35mm: Double? = null, val gpsAltitude: Double? = null,
    val locationCity: String? = null, val locationState: String? = null, val locationCountry: String? = null,
    val videoCodec: String? = null, val keywords: String? = null, val contentHash: String? = null,
    val createdAt: String,
)
@Serializable data class TimelineGroup(val date: String, val media: List<Media>)
@Serializable data class TimelineRequest(
    val cursor: String?, val limit: Int, val groupBy: String, val search: String,
    val mediaType: String?, val classification: String?, val direction: String, val anchorDate: String?,
)
@Serializable data class TimelineResponse(val groups: List<TimelineGroup>, val nextCursor: String?, val previousCursor: String?, val hasOlder: Boolean, val hasNewer: Boolean)

@Serializable data class Album(val id: Long, val name: String, val description: String?, val coverMediaId: Long?, val thumbnailMediaIds: List<Long>, val mediaCount: Long, val createdAt: String)
@Serializable data class AlbumDetail(val id: Long, val name: String, val description: String?, val coverMediaId: Long?, val media: List<Media>, val createdAt: String)
@Serializable data class AlbumsResponse(val albums: List<Album>)
@Serializable data class AlbumIdRequest(val albumId: Long)
@Serializable data class AlbumCreateRequest(val name: String, val description: String?)
@Serializable data class AlbumUpdateRequest(val albumId: Long, val name: String?, val description: String?, val coverMediaId: Long?)
@Serializable data class AlbumMediaRequest(val albumId: Long, val mediaIds: List<Long>)

@Serializable data class Place(val placeId: String, val city: String, val state: String?, val country: String, val mediaCount: Long)
@Serializable data class PageRequest(val cursor: String?, val limit: Int)
@Serializable data class PlaceRequest(val placeId: String, val cursor: String?, val limit: Int)
@Serializable data class PlaceThumbnailRequest(val placeId: String)
@Serializable data class PlaceThumbnailResponse(val thumbnail: String?)
@Serializable data class PlacesResponse(val places: List<Place>, val nextCursor: String?, val hasMore: Boolean)
@Serializable data class PlaceResponse(val place: Place, val media: List<Media>, val nextCursor: String?, val hasMore: Boolean)

@Serializable data class FaceGroup(val faceGroupId: Long, val faceCount: Long, val mediaCount: Long)
@Serializable data class FacesResponse(val groups: List<FaceGroup>, val nextCursor: String?, val hasMore: Boolean)
@Serializable data class FaceGroupRequest(val faceGroupId: Long)
@Serializable data class FaceGroupMediaResponse(val group: FaceGroup, val media: List<Media>)
@Serializable data class FaceMergeRequest(val faceGroupIds: List<Long>)
@Serializable data class FaceMergeResponse(val group: FaceGroup)

@Serializable data class DeduplicateGroup(val clusterId: Long, val items: List<Media>)
@Serializable data class DeduplicateGroupsResponse(val groups: List<DeduplicateGroup>, val nextCursor: String?, val hasMore: Boolean, val totalGroups: Long, val totalMedia: Long)
@Serializable data class TrashMedia(
    val id: Long,
    val filename: String,
    val originalFilename: String,
    val mediaType: String,
    val mimeType: String?,
    val width: Int?,
    val height: Int?,
    val fileSize: Long?,
    val durationSeconds: Double?,
    val dateTaken: String?,
    val deletedAt: String,
    val createdAt: String,
)
@Serializable data class TrashResponse(val items: List<TrashMedia>, val totalCount: Long)
@Serializable data class MediaIdsRequest(val mediaIds: List<Long>)

@Serializable data class BoundingBox(val north: Double, val south: Double, val east: Double, val west: Double)
@Serializable data class MapClustersRequest(val bounds: BoundingBox, val zoom: Int)
@Serializable data class MapMediaRequest(val bounds: BoundingBox, val geohashPrefixes: List<String>)
@Serializable data class MapCluster(val id: String, val lat: Double, val lng: Double, val count: Long, val representativeId: Long)
@Serializable data class MapClustersResponse(val clusters: List<MapCluster>, val totalCount: Long)
@Serializable data class MapMediaResponse(val items: List<Media>)

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

@Serializable data class FeatureFlags(val llm: Boolean, val imageTagging: Boolean, val deduplicate: Boolean, val faceDetection: Boolean, val imageAesthetics: Boolean, val screenshotDetection: Boolean, val documentDetection: Boolean)
@Serializable data class BackupCapabilities(val enabled: Boolean, val protocolVersion: Int, val maxUploadBytes: Long, val maxChunkBytes: Long, val maxActiveUploadsPerUser: Int, val sessionExpiryHours: Long)
@Serializable data class Capabilities(val appVersion: String, val apiVersion: Int, val supportedMediaExtensions: List<String>, val features: FeatureFlags, val backup: BackupCapabilities)
@Serializable data class BackupDeviceRegisterRequest(val deviceId: String, val deviceName: String)
@Serializable data class BackupDeviceRegisterResponse(val registered: Boolean)
@Serializable data class BackupUploadCreateRequest(val protocolVersion: Int, val deviceId: String, val clientAssetId: String, val operationId: String, val originalFilename: String, val mimeType: String, val byteSize: Long, val contentHash: String, val sourceModifiedAt: String, val metadata: JsonObject)
@Serializable data class BackupUploadIdRequest(val uploadId: String)
@Serializable data class BackupUploadResponse(val uploadId: String, val status: String, val uploadedSize: Long, val expectedSize: Long, val contentHash: String?, val mediaId: Long?, val error: String?)

enum class BackupState { QUEUED, UPLOADING, COMPLETING, SERVER_PROCESSING, COMPLETED, FAILED, TERMINAL_FAILED, CANCELLING, CANCELLED }
