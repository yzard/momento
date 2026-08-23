package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.*
import okhttp3.RequestBody
import okhttp3.ResponseBody
import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.Header
import retrofit2.http.POST
import retrofit2.http.PUT
import retrofit2.http.Path

interface MomentoApi {
    @GET("api/v1/client/capabilities") suspend fun capabilities(): Capabilities
    @POST("api/v1/user/authenticate") suspend fun login(@Header("Authorization") basic: String): TokenPair
    @POST("api/v1/user/refresh") suspend fun refresh(@Body request: RefreshTokenRequest): TokenPair
    @POST("api/v1/user/get") suspend fun currentUser(): User
    @POST("api/v1/user/logout") suspend fun logout(@Body request: LogoutRequest): MessageResponse
    @POST("api/v1/user/change-password") suspend fun changePassword(@Body request: ChangePasswordRequest): MessageResponse
    @POST("api/v1/user/list") suspend fun users(): UsersResponse

    @POST("api/v1/timeline/list") suspend fun timeline(@Body request: TimelineRequest): TimelineResponse
    @POST("api/v1/media/search") suspend fun search(@Body request: SearchRequest): SearchResponse
    @POST("api/v1/media/get-batch") suspend fun mediaBatch(@Body request: MediaBatchRequest): MediaListResponse
    @POST("api/v1/album/list") suspend fun albums(): AlbumsResponse
    @POST("api/v1/album/get") suspend fun album(@Body request: AlbumIdRequest): AlbumDetail
    @POST("api/v1/album/create") suspend fun createAlbum(@Body request: AlbumCreateRequest): AlbumDetail
    @POST("api/v1/album/delete") suspend fun deleteAlbum(@Body request: AlbumIdRequest): MessageResponse
    @POST("api/v1/album/update") suspend fun updateAlbum(@Body request: AlbumUpdateRequest): Album
    @POST("api/v1/album/add-media") suspend fun addAlbumMedia(@Body request: AlbumMediaRequest): MessageResponse
    @POST("api/v1/album/remove-media") suspend fun removeAlbumMedia(@Body request: AlbumMediaRequest): MessageResponse
    @POST("api/v1/album/reorder") suspend fun reorderAlbumMedia(@Body request: AlbumMediaRequest): MessageResponse
    @POST("api/v1/places/list") suspend fun places(@Body request: PageRequest): PlacesResponse
    @POST("api/v1/places/get") suspend fun place(@Body request: PlaceRequest): PlaceResponse
    @POST("api/v1/faces/groups/list") suspend fun faces(@Body request: PageRequest): FacesResponse
    @POST("api/v1/faces/groups/get") suspend fun face(@Body request: FaceGroupRequest): FaceGroupMediaResponse
    @POST("api/v1/faces/groups/merge") suspend fun mergeFaces(@Body request: FaceMergeRequest): FaceMergeResponse
    @POST("api/v1/faces/thumbnails/get") suspend fun faceThumbnail(@Body request: FaceGroupRequest): ResponseBody
    @POST("api/v1/ai/deduplicate/groups") suspend fun duplicates(@Body request: PageRequest): DeduplicateGroupsResponse
    @POST("api/v1/ai/deduplicate/start") suspend fun startDuplicates(): MessageResponse
    @POST("api/v1/ai/deduplicate/status") suspend fun duplicateStatus(): DeduplicateStatusResponse
    @POST("api/v1/ai/deduplicate/cancel") suspend fun cancelDuplicates(): DeduplicateActionResponse
    @POST("api/v1/ai/deduplicate/clean") suspend fun cleanDuplicates(): DeduplicateActionResponse
    @POST("api/v1/media/delete") suspend fun moveToTrash(@Body request: MediaIdsRequest): MessageResponse
    @POST("api/v1/trash/list") suspend fun trash(): TrashResponse
    @POST("api/v1/trash/restore") suspend fun restore(@Body request: MediaIdsRequest): MessageResponse
    @POST("api/v1/trash/delete") suspend fun deleteForever(@Body request: MediaIdsRequest): MessageResponse
    @POST("api/v1/trash/empty") suspend fun emptyTrash(): MessageResponse
    @POST("api/v1/map/clusters") suspend fun mapClusters(@Body request: MapClustersRequest): MapClustersResponse
    @POST("api/v1/map/media") suspend fun mapMedia(@Body request: MapMediaRequest): MapMediaResponse

    @POST("api/v1/user/create") suspend fun createUser(@Body request: AdminUserCreateRequest): User
    @POST("api/v1/user/update") suspend fun updateUser(@Body request: AdminUserUpdateRequest): User
    @POST("api/v1/user/delete") suspend fun deleteUser(@Body request: AdminUserIdRequest): MessageResponse
    @POST("api/v1/import/local") suspend fun triggerLocalImport(): MessageResponse
    @POST("api/v1/import/status") suspend fun importStatus(): ImportStatus
    @POST("api/v1/metadata/generate") suspend fun generateMetadata(): JobActionResponse
    @POST("api/v1/metadata/status") suspend fun metadataStatus(): JobStatus
    @POST("api/v1/metadata/reset") suspend fun resetMetadata(): JobActionResponse
    @POST("api/v1/ai/trigger") suspend fun triggerAi(): JobActionResponse
    @POST("api/v1/ai/cancel") suspend fun cancelAi(): JobActionResponse
    @POST("api/v1/ai/clean") suspend fun cleanAi(): JobActionResponse
    @POST("api/v1/ai/ocr/trigger") suspend fun triggerOcr(): JobActionResponse
    @POST("api/v1/ai/ocr/cancel") suspend fun cancelOcr(): JobActionResponse
    @POST("api/v1/ai/ocr/clean") suspend fun cleanOcr(): JobActionResponse
    @POST("api/v1/ai/ocr/status") suspend fun ocrStatus(): JobStatus
    @POST("api/v1/ai/image_tagging/trigger") suspend fun triggerImageTagging(): JobActionResponse
    @POST("api/v1/ai/image_tagging/cancel") suspend fun cancelImageTagging(): JobActionResponse
    @POST("api/v1/ai/image_tagging/clean") suspend fun cleanImageTagging(): JobActionResponse
    @POST("api/v1/ai/image_tagging/status") suspend fun imageTaggingStatus(): JobStatus
    @POST("api/v1/ai/screenshot_detection/status") suspend fun screenshotStatus(): JobStatus
    @POST("api/v1/ai/document_detection/status") suspend fun documentStatus(): JobStatus
    @POST("api/v1/ai/image_aesthetics/status") suspend fun aestheticsStatus(): JobStatus
    @POST("api/v1/ai/faces/status") suspend fun facesStatus(): FaceJobStatus

    @POST("api/v1/backup/device/register") suspend fun registerDevice(@Body request: BackupDeviceRegisterRequest): BackupDeviceRegisterResponse
    @POST("api/v1/backup/upload/create") suspend fun createUpload(@Body request: BackupUploadCreateRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/status") suspend fun uploadStatus(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @PUT("api/v1/backup/upload/chunk/{uploadId}") suspend fun uploadChunk(@Path("uploadId") uploadId: String, @Header("Content-Range") range: String, @Body bytes: RequestBody): BackupUploadResponse
    @POST("api/v1/backup/upload/complete") suspend fun completeUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/cancel") suspend fun cancelUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
}
