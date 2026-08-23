package io.github.yzard.momento.core.data

import io.github.yzard.momento.core.model.*
import io.github.yzard.momento.core.network.MomentoApi
import io.github.yzard.momento.core.network.NetworkClient
import io.github.yzard.momento.core.network.basicAuthorization
import kotlinx.coroutines.flow.first
import okhttp3.RequestBody
import okhttp3.OkHttpClient
import coil.ImageLoader
import android.content.Context
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException
import java.time.Instant

class MomentoRepository(
    private val settingsStore: SettingsStore,
    private val tokenStore: EncryptedTokenStore,
    private val networkClient: NetworkClient,
) {
    private suspend fun api(): MomentoApi = networkClient.api(requireNotNull(settingsStore.settings.first().origin) { "Choose a server first" })

    suspend fun capabilities(origin: String): Capabilities = networkClient.api(origin).capabilities()
    suspend fun login(username: String, password: String): User {
        val service = api()
        tokenStore.saveTokens(service.login(basicAuthorization(username, password)))
        return try {
            service.currentUser()
        } catch (error: IOException) {
            tokenStore.clear()
            throw error
        } catch (error: HttpException) {
            tokenStore.clear()
            throw error
        } catch (error: SerializationException) {
            tokenStore.clear()
            throw error
        }
    }
    fun completeLogin() = tokenStore.markAuthenticated()
    suspend fun currentUser(): User = api().currentUser()
    suspend fun logout() {
        val refresh = tokenStore.refreshToken()
        try {
            if (refresh != null) api().logout(LogoutRequest(refresh))
        } finally {
            tokenStore.clear()
        }
    }
    suspend fun timeline(groupBy: String, mediaType: String?, classification: String?): List<TimelineGroup> = api().timeline(
        timelineRequest(groupBy, mediaType, classification, Instant.now().toString()),
    ).groups
    suspend fun timelinePage(cursor: String?, groupBy: String, mediaType: String?, classification: String?): TimelineResponse = api().timeline(
        TimelineRequest(cursor, 100, groupBy, "", mediaType, classification, "older", Instant.now().toString()),
    )
    suspend fun search(query: String): List<Media> {
        val ids = api().search(SearchRequest(query)).results.map { it.imageId }
        if (ids.isEmpty()) return emptyList()
        return api().mediaBatch(MediaBatchRequest(ids)).items
    }
    suspend fun albums(): List<Album> = api().albums().albums
    suspend fun createAlbum(name: String, description: String?): AlbumDetail = api().createAlbum(AlbumCreateRequest(name, description))
    suspend fun deleteAlbum(id: Long): MessageResponse = api().deleteAlbum(AlbumIdRequest(id))
    suspend fun album(id: Long): AlbumDetail = api().album(AlbumIdRequest(id))
    suspend fun updateAlbum(id: Long, name: String?, description: String?, coverMediaId: Long?): Album = api().updateAlbum(AlbumUpdateRequest(id, name, description, coverMediaId))
    suspend fun addAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().addAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun removeAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().removeAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun reorderAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().reorderAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun places(): List<Place> = api().places(PageRequest(null, 100)).places
    suspend fun place(placeId: String, cursor: String?): PlaceResponse = api().place(PlaceRequest(placeId, cursor, 100))
    suspend fun faces(): List<FaceGroup> = api().faces(PageRequest(null, 100)).groups
    suspend fun faceGroup(id: Long): FaceGroupMediaResponse = api().face(FaceGroupRequest(id))
    suspend fun faceThumbnail(id: Long): ByteArray = api().faceThumbnail(FaceGroupRequest(id)).bytes()
    suspend fun mergeFaces(ids: List<Long>): FaceMergeResponse = api().mergeFaces(FaceMergeRequest(ids))
    suspend fun trash(): List<TrashMedia> = api().trash().items
    suspend fun restore(id: Long): MessageResponse = api().restore(MediaIdsRequest(listOf(id)))
    suspend fun deleteForever(id: Long): MessageResponse = api().deleteForever(MediaIdsRequest(listOf(id)))
    suspend fun emptyTrash(): MessageResponse = api().emptyTrash()
    suspend fun duplicateGroups(): List<DeduplicateGroup> = api().duplicates(PageRequest(null, 100)).groups
    suspend fun startDeduplicate(): MessageResponse = api().startDuplicates()
    suspend fun deduplicateStatus(): DeduplicateStatusResponse = api().duplicateStatus()
    suspend fun cancelDeduplicate(): DeduplicateActionResponse = api().cancelDuplicates()
    suspend fun cleanDeduplicate(): DeduplicateActionResponse = api().cleanDuplicates()
    suspend fun moveToTrash(ids: List<Long>): MessageResponse = api().moveToTrash(MediaIdsRequest(ids))
    suspend fun users(): List<User> = api().users().users
    suspend fun createUser(username: String, email: String, password: String, role: String?): User = api().createUser(AdminUserCreateRequest(username, email, password, role))
    suspend fun updateUser(id: Long, role: String?, active: Boolean?): User = api().updateUser(AdminUserUpdateRequest(id, role, active))
    suspend fun deleteUser(id: Long): MessageResponse = api().deleteUser(AdminUserIdRequest(id))
    suspend fun localImport(): MessageResponse = api().triggerLocalImport()
    suspend fun importStatus(): ImportStatus = api().importStatus()
    suspend fun generateMetadata(): JobActionResponse = api().generateMetadata()
    suspend fun metadataStatus(): JobStatus = api().metadataStatus()
    suspend fun resetMetadata(): JobActionResponse = api().resetMetadata()
    suspend fun triggerAi(): JobActionResponse = api().triggerAi()
    suspend fun cancelAi(): JobActionResponse = api().cancelAi()
    suspend fun cleanAi(): JobActionResponse = api().cleanAi()
    suspend fun triggerOcr(): JobActionResponse = api().triggerOcr()
    suspend fun cancelOcr(): JobActionResponse = api().cancelOcr()
    suspend fun cleanOcr(): JobActionResponse = api().cleanOcr()
    suspend fun ocrStatus(): JobStatus = api().ocrStatus()
    suspend fun triggerImageTagging(): JobActionResponse = api().triggerImageTagging()
    suspend fun cancelImageTagging(): JobActionResponse = api().cancelImageTagging()
    suspend fun cleanImageTagging(): JobActionResponse = api().cleanImageTagging()
    suspend fun imageTaggingStatus(): JobStatus = api().imageTaggingStatus()
    suspend fun mapClusters(bounds: BoundingBox, zoom: Int): MapClustersResponse = api().mapClusters(MapClustersRequest(bounds, zoom))
    suspend fun mapMedia(bounds: BoundingBox, prefixes: List<String>): List<Media> = api().mapMedia(MapMediaRequest(bounds, prefixes)).items
    suspend fun changePassword(current: String, updated: String): MessageResponse = api().changePassword(ChangePasswordRequest(current, updated))
    suspend fun originalUrl(mediaId: Long): String = mediaUrl(mediaId, "original")
    suspend fun previewUrl(mediaId: Long): String = mediaUrl(mediaId, "preview")
    suspend fun thumbnailUrl(mediaId: Long, tiny: Boolean): String = mediaUrl(mediaId, if (tiny) "thumbnail/tiny" else "thumbnail")
    private suspend fun mediaUrl(mediaId: Long, suffix: String): String = "${requireNotNull(settingsStore.settings.first().origin)}/api/v1/media/$mediaId/$suffix"
    fun authorizationHeader(): String? = tokenStore.accessToken()?.let { "Bearer $it" }
    fun authenticatedHttpClient(): OkHttpClient = networkClient.httpClient()
    fun authenticatedImageLoader(context: Context): ImageLoader = networkClient.imageLoader(context)

    suspend fun registerBackupDevice(deviceId: String, deviceName: String): BackupDeviceRegisterResponse = api().registerDevice(BackupDeviceRegisterRequest(deviceId, deviceName))
    suspend fun createBackupUpload(request: BackupUploadCreateRequest): BackupUploadResponse = api().createUpload(request)
    suspend fun backupUploadStatus(uploadId: String): BackupUploadResponse = api().uploadStatus(BackupUploadIdRequest(uploadId))
    suspend fun uploadBackupChunk(uploadId: String, range: String, body: RequestBody): BackupUploadResponse = api().uploadChunk(uploadId, range, body)
    suspend fun completeBackupUpload(uploadId: String): BackupUploadResponse = api().completeUpload(BackupUploadIdRequest(uploadId))
}

fun timelineRequest(groupBy: String, mediaType: String?, classification: String?, anchorDate: String): TimelineRequest =
    TimelineRequest(null, 100, groupBy, "", mediaType, classification, "older", anchorDate)
