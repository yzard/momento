import Foundation
import CryptoKit

struct BackupBatchFile: Sendable {
    let recordId: String
    let filename: String
    let file: URL
    let uploadId: String
    let byteSize: Int64
    let contentHash: String
}

struct BackupBatchPayload: Sendable {
    let file: URL
    let headerBytes: Int64
    let originals: [BackupBatchFile]

    var length: Int64 { originals.reduce(headerBytes) { $0 + $1.byteSize } }

    func fileProgress(sent: Int64) -> (filename: String, finished: Int64, total: Int64)? {
        var remaining = max(0, sent - headerBytes)
        for original in originals {
            if remaining <= original.byteSize {
                return (original.filename, remaining, original.byteSize)
            }
            remaining -= original.byteSize
        }
        return originals.last.map { ($0.filename, $0.byteSize, $0.byteSize) }
    }

    @concurrent static func write(originals: [BackupBatchFile], to destination: URL, limits: BackupBatchCapabilities) async throws -> BackupBatchPayload {
        guard !originals.isEmpty, originals.count <= limits.maxItems else { throw ClientError.message("Invalid backup batch item count.") }
        var total: Int64 = 0
        for original in originals {
            guard original.byteSize > 0, original.byteSize <= Int64.max - total else { throw ClientError.message("Invalid backup batch size.") }
            total += original.byteSize
        }
        guard originals.count == 1 || total <= limits.maxBytes else { throw ClientError.message("Backup batch exceeds its size limit.") }
        let manifest = BackupBatchManifest(version: 1, entries: originals.map {
            BackupBatchEntry(uploadId: $0.uploadId, byteSize: $0.byteSize, contentHash: $0.contentHash)
        })
        let json = try JSONEncoder().encode(manifest)
        guard json.count <= limits.maxManifestBytes, json.count <= Int(UInt32.max) else { throw ClientError.message("Backup batch manifest is too large.") }
        guard FileManager.default.createFile(atPath: destination.path, contents: nil,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]) else {
            throw ClientError.message("Could not create the backup batch staging file.")
        }
        var complete = false
        defer { if !complete { try? FileManager.default.removeItem(at: destination) } }
        let output = try FileHandle(forWritingTo: destination); defer { try? output.close() }
        let count = UInt32(json.count)
        try output.write(contentsOf: Data([UInt8(count >> 24), UInt8((count >> 16) & 255), UInt8((count >> 8) & 255), UInt8(count & 255)]))
        try output.write(contentsOf: json)
        for original in originals {
            try Task.checkCancellation()
            let input = try FileHandle(forReadingFrom: original.file)
            do {
                var copied: Int64 = 0
                var hash = SHA256()
                while let bytes = try input.read(upToCount: 256 * 1024), !bytes.isEmpty {
                    try Task.checkCancellation()
                    guard Int64(bytes.count) <= original.byteSize - copied else { throw ClientError.message("Protected original changed: \(original.filename)") }
                    copied += Int64(bytes.count); hash.update(data: bytes)
                    try output.write(contentsOf: bytes)
                }
                let actual = hash.finalize().map { String(format: "%02x", $0) }.joined()
                guard copied == original.byteSize, actual == original.contentHash else { throw ClientError.message("Protected original changed: \(original.filename)") }
                try input.close()
            } catch { try? input.close(); throw error }
        }
        try output.synchronize()
        complete = true
        return BackupBatchPayload(file: destination, headerBytes: Int64(4 + json.count), originals: originals)
    }
}

func backupBatchCanAppend(sizes: [Int64], next: Int64, limits: BackupBatchCapabilities) -> Bool {
    guard next > 0, sizes.count < limits.maxItems else { return false }
    if sizes.isEmpty { return true }
    guard next <= limits.maxBytes else { return false }
    var remaining = limits.maxBytes - next
    for size in sizes {
        guard size > 0, size <= remaining else { return false }
        remaining -= size
    }
    return true
}
