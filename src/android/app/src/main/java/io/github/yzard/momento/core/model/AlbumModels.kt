package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable

@Serializable data class Album(val id: Long, val name: String, val description: String?, val coverMediaId: Long?, val thumbnailMediaIds: List<Long>, val mediaCount: Long, val createdAt: String)
@Serializable data class AlbumDetail(val id: Long, val name: String, val description: String?, val coverMediaId: Long?, val media: List<Media>, val createdAt: String)
@Serializable data class AlbumsResponse(val albums: List<Album>)
@Serializable data class AlbumIdRequest(val albumId: Long)
@Serializable data class AlbumCreateRequest(val name: String, val description: String?, val mediaIds: List<Long>)
@Serializable data class AlbumUpdateRequest(val albumId: Long, val name: String?, val description: String?, val coverMediaId: Long?)
@Serializable data class AlbumMediaRequest(val albumId: Long, val mediaIds: List<Long>)
