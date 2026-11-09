import Foundation

struct Place: Codable, Sendable {
    var placeId: String
    var city: String
    var state: String?
    var country: String
    var mediaCount: Int64
}

struct PageRequest: Codable, Sendable {
    var cursor: String?
    var limit: Int
}

struct PlaceRequest: Codable, Sendable {
    var placeId: String
    var cursor: String?
    var limit: Int
}

struct PlacesResponse: Codable, Sendable {
    var places: [Place]
    var nextCursor: String?
    var hasMore: Bool
}

struct PlaceResponse: Codable, Sendable {
    var place: Place
    var media: [Media]
    var nextCursor: String?
    var hasMore: Bool
}

struct FaceGroup: Codable, Sendable {
    var faceGroupId: Int64
    var faceCount: Int64
    var mediaCount: Int64
}

struct FacesResponse: Codable, Sendable {
    var groups: [FaceGroup]
    var nextCursor: String?
    var hasMore: Bool
}

struct FaceGroupRequest: Codable, Sendable {
    var faceGroupId: Int64
}

struct FaceGroupMediaResponse: Codable, Sendable {
    var group: FaceGroup
    var media: [Media]
    var faces: [FaceDetection]
}

struct FaceMergeRequest: Codable, Sendable {
    var faceGroupIds: [Int64]
}

struct FaceMergeResponse: Codable, Sendable {
    var group: FaceGroup
}

struct DeduplicateGroup: Codable, Sendable {
    var clusterId: Int64
    var items: [Media]
}

struct DeduplicateGroupsResponse: Codable, Sendable {
    var groups: [DeduplicateGroup]
    var nextCursor: String?
    var hasMore: Bool
    var totalGroups: Int64
    var totalMedia: Int64
}

struct TrashMedia: Codable, Sendable {
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
    var deletedAt: String
    var createdAt: String
}

struct TrashResponse: Codable, Sendable {
    var items: [TrashMedia]
    var totalCount: Int64
}

struct MediaIdsRequest: Codable, Sendable {
    var mediaIds: [Int64]
}

struct BoundingBox: Codable, Sendable {
    var north: Double
    var south: Double
    var east: Double
    var west: Double
}

struct MapClustersRequest: Codable, Sendable {
    var bounds: BoundingBox
    var zoom: Int
}

struct MapMediaRequest: Codable, Sendable {
    var bounds: BoundingBox
    var geohashPrefixes: [String]
}

struct MapCluster: Codable, Sendable {
    var id: String
    var lat: Double
    var lng: Double
    var count: Int64
    var representativeId: Int64
}

struct MapClustersResponse: Codable, Sendable {
    var clusters: [MapCluster]
    var totalCount: Int64
}

struct MapMediaResponse: Codable, Sendable {
    var items: [Media]
}

struct FaceDetection: Codable, Sendable {
    var faceId: Int64
    var mediaId: Int64
    var inputSequence: Int64
    var x: Double
    var y: Double
    var width: Double
    var height: Double
}

struct RejectFacesRequest: Codable, Sendable {
    var requestId: String
    var groupIds: [Int64]
    var faceGroupId: Int64?
    var faceIds: [Int64]
}

struct RejectFacesResponse: Codable, Sendable {
    var rejectedCount: Int
}
