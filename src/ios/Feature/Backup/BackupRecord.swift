import Foundation
import CryptoKit

enum SHA256Key {
    static func value(_ text: String) -> String { digest(Data(text.utf8)) }
    static func digest(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }

    @concurrent static func file(_ url: URL) async throws -> (String, Int64) {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hash = SHA256()
        var size: Int64 = 0
        while let data = try handle.read(upToCount: 1024 * 1024), !data.isEmpty {
            try Task.checkCancellation()
            hash.update(data: data)
            size += Int64(data.count)
        }
        return (hash.finalize().map { String(format: "%02x", $0) }.joined(), size)
    }
}

enum BackupState: String, Codable, Sendable {
    case queued, uploading, completing, serverProcessing, completed, failed, terminalFailed, cancelling, cancelled, excluded
    var active: Bool { ![.completed, .terminalFailed, .cancelled, .excluded].contains(self) }
}

struct BackupRecord: Codable, Identifiable, Sendable {
    var id: String
    var assetId: String
    var resourceIndex: Int
    var filename: String
    var mimeType: String
    var modifiedAt: String
    var operationId: String
    var uploadId: String?
    var state: BackupState
    var byteSize: Int64
    var uploadedSize: Int64
    var contentHash: String?
    var serverHash: String?
    var metadata: [String: JSONValue]
    var error: String?
    var cameraCapture: Bool?

    mutating func accept(_ response: BackupUploadResponse) throws {
        guard !response.uploadId.isEmpty, uploadId == nil || uploadId == response.uploadId else {
            throw ClientError.message("Server response belongs to a different backup upload.")
        }
        guard response.expectedSize == byteSize, response.uploadedSize >= 0, response.uploadedSize <= byteSize else {
            throw ClientError.message("Server returned an invalid upload offset or size.")
        }
        if let hash = response.contentHash, hash != contentHash { throw ClientError.message("Server upload hash does not match the original.") }
        let acceptedState: BackupState
        switch response.status {
        case "completed":
            guard let contentHash, contentHash == response.contentHash, response.uploadedSize == byteSize else {
                throw ClientError.message("Completed backup is missing original-file verification.")
            }
            acceptedState = .completed
        case "processing", "queued": acceptedState = .serverProcessing
        case "failed": acceptedState = .terminalFailed
        case "cancelled", "expired": acceptedState = .cancelled
        case "uploading", "writing": acceptedState = .uploading
        case "cancelling": acceptedState = .cancelling
        case "completing": acceptedState = .completing
        default: throw ClientError.message("Unknown server backup state: \(response.status)")
        }
        uploadId = response.uploadId
        uploadedSize = response.uploadedSize
        serverHash = response.contentHash
        state = acceptedState
        error = response.error
    }
}

actor BackupStore {
    let root: URL
    init(root: URL) throws {
        self.root = root
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        var excluded = root
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try excluded.setResourceValues(values)
    }

    func records() throws -> [BackupRecord] {
        try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "json" }
            .map { try JSONDecoder().decode(BackupRecord.self, from: Data(contentsOf: $0)) }
            .sorted { $0.id < $1.id }
    }
    func save(_ record: BackupRecord) throws {
        var record = record
        let file = root.appending(path: record.id + ".json")
        if FileManager.default.fileExists(atPath: file.path) {
            let existing = try JSONDecoder().decode(BackupRecord.self, from: Data(contentsOf: file))
            if existing.state == .cancelling && record.state.active { record.state = .cancelling }
        }
        try JSONEncoder().encode(record).write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }
    func requestCancellation() throws {
        for var record in try records() where record.state.active { record.state = .cancelling; try save(record) }
    }
    func batchFile() -> URL { root.appending(path: UUID().uuidString + ".batch-body") }
    func cleanBatchFiles() throws {
        for file in try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil) where file.pathExtension == "batch-body" {
            try FileManager.default.removeItem(at: file)
        }
    }
    func snapshot(_ record: BackupRecord) -> URL { root.appending(path: record.operationId + ".original") }
    func removeSnapshot(_ record: BackupRecord) throws {
        let url = snapshot(record)
        if FileManager.default.fileExists(atPath: url.path) { try FileManager.default.removeItem(at: url) }
    }
    func clear() throws {
        let all = try records()
        guard !all.contains(where: { $0.state.active }) else { throw ClientError.message("Finish or cancel active backups before clearing history.") }
        for record in all {
            try removeSnapshot(record)
            try FileManager.default.removeItem(at: root.appending(path: record.id + ".json"))
        }
    }
}
