package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable

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

