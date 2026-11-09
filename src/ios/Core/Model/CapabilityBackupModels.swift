import Foundation

struct FeatureFlags: Codable, Sendable {
    var llm: Bool
    var imageTagging: Bool
    var deduplicate: Bool
    var faceDetection: Bool
    var imageAesthetics: Bool
    var screenshotDetection: Bool
    var documentDetection: Bool
}

struct BackupCapabilities: Codable, Sendable {
    var enabled: Bool
    var protocolVersion: Int
    var maxUploadBytes: Int64
    var maxChunkBytes: Int64
    var sessionExpiryHours: Int64
    var batch: BackupBatchCapabilities?
}

struct BackupBatchCapabilities: Codable, Sendable {
    var version: Int
    var maxBytes: Int64
    var maxItems: Int
    var maxManifestBytes: Int
}

struct BackupBatchPrepareRequest: Codable, Sendable { var uploads: [BackupUploadCreateRequest] }
struct BackupUploadIdsRequest: Codable, Sendable { var uploadIds: [String] }
struct BackupBatchEntry: Codable, Sendable { var uploadId: String; var byteSize: Int64; var contentHash: String }
struct BackupBatchManifest: Codable, Sendable { var version: Int; var entries: [BackupBatchEntry] }
struct BackupBatchResponse: Codable, Sendable {
    var results: [BackupBatchResult]
    func indexed(keys: [String]) throws -> [String: BackupBatchResult] {
        let expected = Set(keys)
        guard expected.count == keys.count, results.count == keys.count, Set(results.map(\.key)) == expected,
              results.allSatisfy({ ($0.upload != nil) != ($0.error != nil) }) else {
            throw ClientError.message("Server returned an invalid backup batch response.")
        }
        return Dictionary(uniqueKeysWithValues: results.map { ($0.key, $0) })
    }
}
struct BackupBatchResult: Codable, Sendable { var key: String; var upload: BackupUploadResponse?; var error: BackupBatchError? }
struct BackupBatchError: Codable, Sendable { var status: Int; var detail: String }

struct Capabilities: Codable, Sendable {
    var appVersion: String
    var apiVersion: Int
    var supportedMediaExtensions: [String]
    var features: FeatureFlags
    var backup: BackupCapabilities
}

struct BackupDeviceRegisterRequest: Codable, Sendable {
    var deviceId: String
    var deviceName: String
}

struct BackupDeviceRegisterResponse: Codable, Sendable {
    var registered: Bool
}

struct BackupUploadCreateRequest: Codable, Sendable {
    var protocolVersion: Int
    var deviceId: String
    var clientAssetId: String
    var operationId: String
    var originalFilename: String
    var mimeType: String
    var byteSize: Int64
    var contentHash: String
    var sourceModifiedAt: String
    var metadata: [String: JSONValue]
}

struct BackupUploadIdRequest: Codable, Sendable {
    var uploadId: String
}

struct BackupUploadResponse: Codable, Sendable {
    var uploadId: String
    var status: String
    var uploadedSize: Int64
    var expectedSize: Int64
    var contentHash: String?
    var mediaId: Int64?
    var error: String?
}
