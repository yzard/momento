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
    @POST("api/v1/places/thumbnail") suspend fun placeThumbnail(@Body request: PlaceThumbnailRequest): PlaceThumbnailResponse
    @POST("api/v1/faces/groups/list") suspend fun faces(@Body request: PageRequest): FacesResponse
    @POST("api/v1/faces/groups/get") suspend fun face(@Body request: FaceGroupRequest): FaceGroupMediaResponse
    @POST("api/v1/faces/groups/merge") suspend fun mergeFaces(@Body request: FaceMergeRequest): FaceMergeResponse
    @POST("api/v1/faces/thumbnails/get") suspend fun faceThumbnail(@Body request: FaceGroupRequest): ResponseBody
    @POST("api/v1/duplicates/list") suspend fun duplicates(@Body request: PageRequest): DeduplicateGroupsResponse
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
    @POST("api/v1/backup/device/register") suspend fun registerDevice(@Body request: BackupDeviceRegisterRequest): BackupDeviceRegisterResponse
    @POST("api/v1/backup/upload/create") suspend fun createUpload(@Body request: BackupUploadCreateRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/status") suspend fun uploadStatus(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @PUT("api/v1/backup/upload/chunk/{uploadId}") suspend fun uploadChunk(@Path("uploadId") uploadId: String, @Header("Content-Range") range: String, @Header("X-Content-SHA256") contentHash: String, @Body bytes: RequestBody): BackupUploadResponse
    @POST("api/v1/backup/upload/complete") suspend fun completeUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
    @POST("api/v1/backup/upload/cancel") suspend fun cancelUpload(@Body request: BackupUploadIdRequest): BackupUploadResponse
}
