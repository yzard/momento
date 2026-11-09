import Foundation

struct TokenPair: Codable, Sendable {
    var accessToken: String
    var refreshToken: String
    var tokenType: String
}

struct RefreshTokenRequest: Codable, Sendable {
    var refreshToken: String
}

struct LogoutRequest: Codable, Sendable {
    var refreshToken: String
}

struct ChangePasswordRequest: Codable, Sendable {
    var currentPassword: String
    var newPassword: String
}

struct MessageResponse: Codable, Sendable {
    var message: String
    var status: String?
}

struct User: Codable, Sendable {
    var id: Int64
    var username: String
    var email: String
    var role: String
    var isReserved: Bool
    var mustChangePassword: Bool
    var isActive: Bool
    var createdAt: String
}

struct UsersResponse: Codable, Sendable {
    var users: [User]
}

struct Media: Codable, Sendable {
    var id: Int64
    var filename: String
    var originalFilename: String
    var mediaType: String
    var mimeType: String?
    var width: Int?
    var height: Int?
    var fileSize: Int64?
    var durationSeconds: Double?
    var dateTaken: String?
    var gpsLatitude: Double?
    var gpsLongitude: Double?
    var cameraMake: String?
    var cameraModel: String?
    var lensMake: String?
    var lensModel: String?
    var iso: Int?
    var exposureTime: String?
    var fNumber: Double?
    var focalLength: Double?
    var focalLength35mm: Double?
    var gpsAltitude: Double?
    var locationCity: String?
    var locationState: String?
    var locationCountry: String?
    var videoCodec: String?
    var keywords: String?
    var contentHash: String?
    var createdAt: String
}

struct TimelineGroup: Codable, Sendable {
    var date: String
    var media: [Media]
}

struct TimelineRequest: Codable, Sendable {
    var cursor: String?
    var limit: Int
    var groupBy: String
    var search: String
    var mediaType: String?
    var classification: String?
    var direction: String
    var anchorDate: String?
}

struct TimelineResponse: Codable, Sendable {
    var groups: [TimelineGroup]
    var nextCursor: String?
    var previousCursor: String?
    var hasOlder: Bool
    var hasNewer: Bool
}
