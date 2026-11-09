import Foundation

struct Album: Codable, Sendable {
    var id: Int64
    var name: String
    var description: String?
    var coverMediaId: Int64?
    var thumbnailMediaIds: [Int64]
    var mediaCount: Int64
    var createdAt: String
}

struct AlbumDetail: Codable, Sendable {
    var id: Int64
    var name: String
    var description: String?
    var coverMediaId: Int64?
    var media: [Media]
    var createdAt: String
}

struct AlbumsResponse: Codable, Sendable {
    var albums: [Album]
}

struct AlbumIdRequest: Codable, Sendable {
    var albumId: Int64
}

struct AlbumCreateRequest: Codable, Sendable {
    var name: String
    var description: String?
    var mediaIds: [Int64]
}

struct AlbumUpdateRequest: Codable, Sendable {
    var albumId: Int64
    var name: String?
    var description: String?
    var coverMediaId: Int64?
}

struct AlbumMediaRequest: Codable, Sendable {
    var albumId: Int64
    var mediaIds: [Int64]
}
