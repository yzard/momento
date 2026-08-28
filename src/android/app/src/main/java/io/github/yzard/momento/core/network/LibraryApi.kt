package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.*
import okhttp3.ResponseBody
import retrofit2.http.Body
import retrofit2.http.POST

interface LibraryApi {
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
}
