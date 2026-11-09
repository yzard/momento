import Foundation
import Photos
import Testing
@testable import Momento

@MainActor struct PhotoOriginalDeviceAuditTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["MOMENTO_PHYSICAL_READONLY"] == "1"
        && ProcessInfo.processInfo.environment["MOMENTO_BACKUP_DIAGNOSTIC_ASSETS"]?.hasPrefix("[") == true))
    func auditsSelectedBackupFailuresWithoutUploadingOrReadingEditedResources() async throws {
        let raw = try #require(ProcessInfo.processInfo.environment["MOMENTO_BACKUP_DIAGNOSTIC_ASSETS"])
        let identifiers = try JSONDecoder().decode([String].self, from: Data(raw.utf8))
        try #require(!identifiers.isEmpty && identifiers.count <= 10)
        let session = try #require(try SessionVault().read())
        let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
        let account = SHA256Key.value(session.origin.absoluteString + ":" + String(session.user.id))
        let store = try BackupStore(root: base.appending(path: "backup/\(account)"))
        let records = try await store.records()
        let root = FileManager.default.temporaryDirectory.appending(path: "original-audit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var observations: [[String: String]] = []
        for id in identifiers {
            let record = try #require(records.first { $0.assetId == id && $0.state == .failed } ?? records.first { $0.assetId == id })
            let asset = try #require(PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject)
            let resources = PHAssetResource.assetResources(for: asset)
            var observation = ["filename": record.filename, "previousError": record.error ?? "", "scannedModifiedAt": record.modifiedAt,
                "currentModifiedAt": (asset.modificationDate ?? asset.creationDate ?? .distantPast).ISO8601Format(),
                "resources": resources.map { "\($0.type.rawValue):\($0.originalFilename)" }.joined(separator: ",")]
            do {
                let file = root.appending(path: UUID().uuidString + ".original")
                try await PhotoLibrary.export(record, to: file, cellular: false)
                let (hash, size) = try await SHA256Key.file(file)
                observation["export"] = "\(size) bytes; SHA256 \(hash)"
                observation["cameraMetadata"] = String(try await PhotoLibrary.cameraOriginal(file, mimeType: record.mimeType))
            } catch { observation["exportError"] = error.localizedDescription }
            if observation["exportError"] != nil && asset.mediaType == .image {
                let options = PHContentEditingInputRequestOptions(); options.isNetworkAccessAllowed = true; options.canHandleAdjustmentData = { _ in true }
                do {
                    let file: URL = try await withCheckedThrowingContinuation { continuation in
                        asset.requestContentEditingInput(with: options) { input, info in
                            if let url = input?.fullSizeImageURL { continuation.resume(returning: url) }
                            else { continuation.resume(throwing: info[PHContentEditingInputErrorKey] as? Error ?? ClientError.message("Original editing input unavailable")) }
                        }
                    }
                    let (hash,size) = try await SHA256Key.file(file)
                    observation["originalEditingInput"] = "\(size) bytes; SHA256 \(hash)"
                } catch { observation["originalEditingInputError"] = String(reflecting: error as NSError) }
                do {
                    let options = PHImageRequestOptions(); options.version = .original; options.isNetworkAccessAllowed = true; options.deliveryMode = .highQualityFormat
                    let data: Data = try await withCheckedThrowingContinuation { continuation in
                        PHImageManager.default().requestImageDataAndOrientation(for: asset, options: options) { bytes, _, _, info in
                            if let bytes { continuation.resume(returning: bytes) }
                            else { continuation.resume(throwing: info?[PHImageErrorKey] as? Error ?? ClientError.message("Original image data unavailable")) }
                        }
                    }
                    observation["originalImageData"] = "\(data.count) bytes; SHA256 \(SHA256Key.digest(data))"
                } catch { observation["originalImageDataError"] = String(reflecting: error as NSError) }
            }
            observations.append(observation)
        }
        let data = try JSONSerialization.data(withJSONObject: observations, options: [.prettyPrinted,.sortedKeys])
        print("Momento unedited-original audit: \(String(decoding: data, as: UTF8.self))")
        try #require(observations.count == identifiers.count)
    }
}
