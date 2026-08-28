package io.github.yzard.momento.core.model

import kotlinx.serialization.Serializable

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
