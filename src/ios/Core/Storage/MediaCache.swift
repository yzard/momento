import Foundation
import CryptoKit

actor MediaCache {
    private let root: URL
    private var limit: Int

    init(root: URL, limit: Int) throws {
        self.root = root
        self.limit = limit
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func setLimit(_ value: Int) throws { limit = value; try evict() }

    func data(path: String, api: APIClient) async throws -> Data {
        let key = SHA256.hash(data: Data(path.utf8)).map { String(format: "%02x", $0) }.joined()
        let file = root.appending(path: key)
        if FileManager.default.fileExists(atPath: file.path) {
            let data = try Data(contentsOf: file)
            try FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: file.path)
            return data
        }
        let data = try await api.bytes(path: path, method: "GET", body: nil, contentType: nil, headers: [:], cellular: true)
        if limit > 0 && data.count <= limit {
            try data.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            try evict()
        }
        return data
    }

    func clear() throws {
        for file in try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil) {
            try FileManager.default.removeItem(at: file)
        }
    }

    private func evict() throws {
        let files = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey])
        let entries = try files.map { file in (file, try file.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])) }
            .sorted { ($0.1.contentModificationDate ?? .distantPast) < ($1.1.contentModificationDate ?? .distantPast) }
        var total = entries.reduce(0) { $0 + ($1.1.fileSize ?? 0) }
        for (file, values) in entries where total > limit {
            try FileManager.default.removeItem(at: file)
            total -= values.fileSize ?? 0
        }
    }
}
