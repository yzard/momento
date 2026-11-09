import Foundation
import Photos

/// An unedited resource has its own identity. Asset modification dates also change for
/// metadata/synchronization updates; array positions can change when Photos adds resources.
struct PhotoOriginalIdentity: Hashable, Sendable {
    let assetId: String
    let filename: String
    let type: Int

    static let uneditedTypes: Set<Int> = [PHAssetResourceType.photo.rawValue, PHAssetResourceType.video.rawValue,
        PHAssetResourceType.alternatePhoto.rawValue, PHAssetResourceType.pairedVideo.rawValue]

    init?(record: BackupRecord) {
        guard case .object(let photoKit) = record.metadata["photoKit"],
              case .number(let number) = photoKit["resourceType"], let type = Int(exactly: number),
              Self.uneditedTypes.contains(type) else { return nil }
        self.assetId = record.assetId; self.filename = record.filename; self.type = type
    }
    init(assetId: String, filename: String, type: Int) {
        self.assetId = assetId; self.filename = filename; self.type = type
    }
    func recordId(uniformType: String) -> String {
        SHA256Key.value("photoKitOriginal:" + [assetId, filename, String(type), uniformType].map { "\($0.utf8.count):\($0)" }.joined())
    }
}

struct PhotoOriginalResource: Sendable {
    let type: Int
    let filename: String
    let uniformType: String

    static func select(for record: BackupRecord, from resources: [Self]) throws -> Int {
        let identity = PhotoOriginalIdentity(record: record)
        let contentType: String?
        if case .object(let photoKit) = record.metadata["photoKit"], case .string(let value) = photoKit["uniformTypeIdentifier"] { contentType = value }
        else { contentType = nil }
        let matches = resources.indices.filter { index in
            let resource = resources[index]
            return PhotoOriginalIdentity.uneditedTypes.contains(resource.type) && resource.filename == record.filename
                && (identity == nil || resource.type == identity!.type)
                && (contentType == nil || resource.uniformType == contentType)
        }
        guard matches.count == 1, let index = matches.first else {
            throw ClientError.message(matches.isEmpty
                ? "The unedited original resource \(record.filename) is not available in Photos."
                : "Photos returned multiple matching unedited originals for \(record.filename); cannot select one safely.")
        }
        return index
    }
}

extension BackupRecord {
    var duplicateOriginalOf: String? {
        if case .string(let value) = metadata["duplicateOriginalOf"] { return value }
        return nil
    }
}

/// Retain the durable operation and never mark a duplicate as uploaded. Only redundant,
/// unprepared local discovery records are excluded; remote/hashed transfers remain intact.
func duplicateOriginalUpdates(_ records: [BackupRecord]) -> [BackupRecord] {
    let eligible = records.filter { $0.duplicateOriginalOf == nil && PhotoOriginalIdentity(record: $0) != nil }
    let groups = Dictionary(grouping: eligible, by: { PhotoOriginalIdentity(record: $0)! })
    func rank(_ record: BackupRecord) -> Int {
        if record.state == .completed && record.contentHash != nil && record.contentHash == record.serverHash { return 0 }
        if record.uploadId != nil { return 1 }
        if record.contentHash != nil { return 2 }
        if record.state == .failed { return 3 }
        return 4
    }
    var updates: [BackupRecord] = []
    for group in groups.values where group.count > 1 {
        let contentTypes = Set(group.compactMap { record -> String? in
            if case .object(let value) = record.metadata["photoKit"], case .string(let type) = value["uniformTypeIdentifier"] { return type }
            return nil
        })
        guard contentTypes.count <= 1, let canonical = group.min(by: { a, b in
            rank(a) == rank(b) ? a.id < b.id : rank(a) < rank(b)
        }) else { continue }
        for var record in group where record.id != canonical.id && record.uploadId == nil && record.contentHash == nil
            && record.byteSize == 0 && (record.state == .queued || record.state == .failed) {
            record.state = .excluded
            record.error = nil
            record.metadata["duplicateOriginalOf"] = .string(canonical.id)
            updates.append(record)
        }
    }
    return updates
}
