package io.github.yzard.momento.core.data

import io.github.yzard.momento.core.model.*
import io.github.yzard.momento.core.network.MomentoApi
import io.github.yzard.momento.core.network.NetworkClient
import io.github.yzard.momento.core.network.basicAuthorization
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.RequestBody
import okhttp3.Request
import okhttp3.OkHttpClient
import coil.ImageLoader
import android.content.Context
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException
import java.io.File

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
    fun requirePasswordChange() = tokenStore.markAuthenticationIncomplete()
    suspend fun currentUser(): User = api().currentUser()
    suspend fun logout() {
        val refresh = tokenStore.refreshToken()
        try {
            if (refresh != null) api().logout(LogoutRequest(refresh))
        } finally {
            tokenStore.clear()
        }
    }
    suspend fun timelinePage(
        cursor: String?,
        groupBy: String,
        search: String,
        mediaType: String?,
        classification: String?,
        direction: String,
        anchorDate: String,
    ): TimelineResponse = api().timeline(
        timelineRequest(cursor, groupBy, search, mediaType, classification, direction, anchorDate),
    )
    suspend fun albums(): List<Album> = api().albums().albums
    suspend fun createAlbum(name: String, description: String?): AlbumDetail = api().createAlbum(AlbumCreateRequest(name, description))
    suspend fun deleteAlbum(id: Long): MessageResponse = api().deleteAlbum(AlbumIdRequest(id))
    suspend fun album(id: Long): AlbumDetail = api().album(AlbumIdRequest(id))
    suspend fun updateAlbum(id: Long, name: String?, description: String?, coverMediaId: Long?): Album = api().updateAlbum(AlbumUpdateRequest(id, name, description, coverMediaId))
    suspend fun addAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().addAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun removeAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().removeAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun reorderAlbumMedia(id: Long, mediaIds: List<Long>): MessageResponse = api().reorderAlbumMedia(AlbumMediaRequest(id, mediaIds))
    suspend fun places(cursor: String?): PlacesResponse = api().places(pagedListRequest(cursor))
    suspend fun place(placeId: String, cursor: String?): PlaceResponse = api().place(PlaceRequest(placeId, cursor, 100))
    suspend fun placeThumbnail(placeId: String): String? = api().placeThumbnail(PlaceThumbnailRequest(placeId)).thumbnail
    suspend fun faces(cursor: String?): FacesResponse = api().faces(pagedListRequest(cursor))
    suspend fun faceGroup(id: Long): FaceGroupMediaResponse = api().face(FaceGroupRequest(id))
    suspend fun faceThumbnail(id: Long): ByteArray = api().faceThumbnail(FaceGroupRequest(id)).bytes()
    suspend fun mergeFaces(ids: List<Long>): FaceMergeResponse = api().mergeFaces(FaceMergeRequest(ids))
    suspend fun trash(): List<TrashMedia> = api().trash().items
    suspend fun restore(ids: List<Long>): MessageResponse = api().restore(MediaIdsRequest(ids))
    suspend fun deleteForever(ids: List<Long>): MessageResponse = api().deleteForever(MediaIdsRequest(ids))
    suspend fun emptyTrash(): MessageResponse = api().emptyTrash()
    suspend fun duplicateGroups(cursor: String?): DeduplicateGroupsResponse = api().duplicates(PageRequest(cursor, 20))
    suspend fun moveToTrash(ids: List<Long>): MessageResponse = api().moveToTrash(MediaIdsRequest(ids))
    suspend fun users(): List<User> = api().users().users
    suspend fun createUser(username: String, email: String, password: String, role: String?): User = api().createUser(AdminUserCreateRequest(username, email, password, role))
    suspend fun updateUser(id: Long, role: String?, active: Boolean?): User = api().updateUser(AdminUserUpdateRequest(id, role, active))
    suspend fun deleteUser(id: Long): MessageResponse = api().deleteUser(AdminUserIdRequest(id))
    suspend fun localImport(): MessageResponse = api().triggerLocalImport()
    suspend fun importStatus(): ImportStatus = api().importStatus()
    suspend fun generateMetadata(): JobActionResponse = api().generateMetadata(EmptyRequest())
    suspend fun metadataStatus(): JobStatus = api().metadataStatus(EmptyRequest())
    suspend fun resetMetadata(): JobActionResponse = api().resetMetadata(EmptyRequest())
    suspend fun startAi(): AiActionResponse = api().startAi()
    suspend fun aiStatus(): AiStatusResponse = api().aiStatus()
    suspend fun cancelAi(): AiActionResponse = api().cancelAi()
    suspend fun cleanAi(): AiActionResponse = api().cleanAi()
    suspend fun updateAiSchedule(feature: String, cronExpression: String): AiFeatureSchedule =
        api().updateAiSchedule(AiScheduleUpdateRequest(feature, cronExpression))
    suspend fun startAiFeature(feature: String): AiActionResponse = api().startAiFeature(feature)
    suspend fun cancelAiFeature(feature: String): AiActionResponse = api().cancelAiFeature(feature)
    suspend fun cleanAiFeature(feature: String): AiActionResponse = api().cleanAiFeature(feature)
    suspend fun mapClusters(bounds: BoundingBox, zoom: Int): MapClustersResponse = api().mapClusters(MapClustersRequest(bounds, zoom))
    suspend fun mapMedia(bounds: BoundingBox, prefixes: List<String>): List<Media> = api().mapMedia(MapMediaRequest(bounds, prefixes)).items
    suspend fun changePassword(current: String, updated: String): MessageResponse {
        val response = api().changePassword(ChangePasswordRequest(current, updated))
        tokenStore.clear()
        return response
    }
    suspend fun originalUrl(mediaId: Long): String = mediaUrl(mediaId, "original")
    suspend fun previewUrl(mediaId: Long): String = mediaUrl(mediaId, "preview")
    suspend fun thumbnailUrl(mediaId: Long, tiny: Boolean): String = mediaUrl(mediaId, if (tiny) "thumbnail/tiny" else "thumbnail")
    suspend fun trashThumbnailUrl(mediaId: Long): String =
        "${requireNotNull(settingsStore.settings.first().origin)}/api/v1/trash/$mediaId/thumbnail/tiny"
    private suspend fun mediaUrl(mediaId: Long, suffix: String): String = "${requireNotNull(settingsStore.settings.first().origin)}/api/v1/media/$mediaId/$suffix"
    fun authorizationHeader(): String? = tokenStore.accessToken()?.let { "Bearer $it" }
    fun authenticatedHttpClient(): OkHttpClient = networkClient.httpClient()
    fun authenticatedImageLoader(context: Context): ImageLoader = networkClient.imageLoader(context)
    suspend fun downloadOriginal(mediaId: Long, destination: File) =
        downloadToFile(originalUrl(mediaId), destination)
    suspend fun downloadAndroidApk(destination: File) {
        val origin = requireNotNull(settingsStore.settings.first().origin).trimEnd('/')
        downloadToFile("$origin/momento-android.apk", destination)
    }
    private suspend fun downloadToFile(url: String, destination: File) = withContext(Dispatchers.IO) {
        val parent = destination.parentFile
        if (parent == null || (!parent.isDirectory && !parent.mkdirs())) {
            throw IOException("Could not create the download cache")
        }
        val request = Request.Builder().url(url).build()
        authenticatedHttpClient().newCall(request).execute().use { response ->
            if (!response.isSuccessful) throw IOException("Download failed with status ${response.code}")
            val body = response.body ?: throw IOException("Download returned no content")
            body.byteStream().use { input ->
                destination.outputStream().use { output -> input.copyTo(output) }
            }
        }
    }

    suspend fun registerBackupDevice(deviceId: String, deviceName: String): BackupDeviceRegisterResponse = api().registerDevice(BackupDeviceRegisterRequest(deviceId, deviceName))
    suspend fun createBackupUpload(request: BackupUploadCreateRequest): BackupUploadResponse = api().createUpload(request)
    suspend fun backupUploadStatus(uploadId: String): BackupUploadResponse = api().uploadStatus(BackupUploadIdRequest(uploadId))
    suspend fun uploadBackupChunk(uploadId: String, range: String, body: RequestBody): BackupUploadResponse = api().uploadChunk(uploadId, range, body)
    suspend fun completeBackupUpload(uploadId: String): BackupUploadResponse = api().completeUpload(BackupUploadIdRequest(uploadId))
    suspend fun cancelBackupUpload(uploadId: String): BackupUploadResponse = api().cancelUpload(BackupUploadIdRequest(uploadId))
}

fun timelineRequest(
    cursor: String?,
    groupBy: String,
    search: String,
    mediaType: String?,
    classification: String?,
    direction: String,
    anchorDate: String,
): TimelineRequest = TimelineRequest(
    cursor,
    100,
    groupBy,
    search,
    mediaType,
    classification,
    direction,
    anchorDate,
)

fun pagedListRequest(cursor: String?): PageRequest = PageRequest(cursor, 100)
