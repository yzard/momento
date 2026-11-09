import Foundation
import Photos
import Testing
@testable import Momento

@MainActor struct PhotoLibraryDeviceTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["MOMENTO_PHYSICAL_READONLY"] == "1"
        && ProcessInfo.processInfo.environment["MOMENTO_PHOTO_DIAGNOSTIC_ASSET"]?.hasPrefix("$(") == false
        && ProcessInfo.processInfo.environment["MOMENTO_PHOTO_DIAGNOSTIC_ASSET"]?.isEmpty == false))
    func readsAnExplicitlySelectedOriginalWithoutUploading() async throws {
        let identifier = try #require(ProcessInfo.processInfo.environment["MOMENTO_PHOTO_DIAGNOSTIC_ASSET"])
        try #require(!identifier.isEmpty && !identifier.hasPrefix("$("))
        let asset = try #require(PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil).firstObject)
        try #require(asset.mediaType == .image && asset.pixelWidth * asset.pixelHeight <= 25_000_000)
        let resources = PHAssetResource.assetResources(for: asset)
        let index = try #require(resources.firstIndex { $0.type == .photo })
        var record = BackupTests().record()
        record.assetId = identifier; record.resourceIndex = index
        record.filename = resources[index].originalFilename
        record.modifiedAt = (asset.modificationDate ?? asset.creationDate ?? Date(timeIntervalSince1970: 0)).ISO8601Format()
        let root = FileManager.default.temporaryDirectory.appending(path: "photo-readonly-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var observations: [String: String] = ["filename": record.filename,
            "resources": resources.map { "\($0.type.rawValue):\($0.uniformTypeIdentifier)" }.joined(separator: ",")]
        do {
            let snapshot = root.appending(path: "original.snapshot")
            try await PhotoLibrary.export(record, to: snapshot, cellular: false)
            let (hash, size) = try await SHA256Key.file(snapshot)
            observations["resourceExport"] = "\(size) bytes; SHA256 \(hash)"
        } catch { observations["resourceExportError"] = String(reflecting: error as NSError) }
        do {
            let options = PHContentEditingInputRequestOptions()
            options.isNetworkAccessAllowed = true
            options.canHandleAdjustmentData = { _ in true }
            let input: (URL, String) = try await withCheckedThrowingContinuation { continuation in
                asset.requestContentEditingInput(with: options) { input, info in
                    if let input, let url = input.fullSizeImageURL { continuation.resume(returning: (url, input.contentType?.identifier ?? "unknown")) }
                    else { continuation.resume(throwing: info[PHContentEditingInputErrorKey] as? Error ?? ClientError.message("Photos did not provide an original editing input")) }
                }
            }
            let (hash, size) = try await SHA256Key.file(input.0)
            observations["originalEditingInput"] = "\(input.1); \(size) bytes; SHA256 \(hash)"
        } catch { observations["originalEditingInputError"] = String(reflecting: error as NSError) }
        do {
            let options = PHImageRequestOptions()
            options.isNetworkAccessAllowed = true
            options.version = .original
            options.deliveryMode = .highQualityFormat
            let data: Data = try await withCheckedThrowingContinuation { continuation in
                PHImageManager.default().requestImageDataAndOrientation(for: asset, options: options) { bytes, _, _, info in
                    if let bytes { continuation.resume(returning: bytes) }
                    else { continuation.resume(throwing: info?[PHImageErrorKey] as? Error ?? ClientError.message("Photos did not provide original image data")) }
                }
            }
            observations["originalImageData"] = "\(data.count) bytes; SHA256 \(SHA256Key.digest(data))"
        } catch { observations["originalImageDataError"] = String(reflecting: error as NSError) }
        if let edited = resources.first(where: { $0.type == .fullSizePhoto }) {
            do {
                let file = root.appending(path: "edited-resource.snapshot")
                let options = PHAssetResourceRequestOptions(); options.isNetworkAccessAllowed = true
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                    PHAssetResourceManager.default().writeData(for: edited, toFile: file, options: options) { error in
                        if let error { continuation.resume(throwing: error) } else { continuation.resume() }
                    }
                }
                let (hash, size) = try await SHA256Key.file(file)
                let camera = try await PhotoLibrary.cameraOriginal(file, mimeType: "image/jpeg")
                observations["editedResource"] = "\(edited.originalFilename); \(size) bytes; SHA256 \(hash); camera metadata \(camera)"
            } catch { observations["editedResourceError"] = String(reflecting: error as NSError) }
        }
        let report = try JSONSerialization.data(withJSONObject: observations, options: [.prettyPrinted, .sortedKeys])
        print("Momento read-only PhotoKit diagnostic: \(String(decoding: report, as: UTF8.self))")
        let expectation = try #require(ProcessInfo.processInfo.environment["MOMENTO_PHOTO_DIAGNOSTIC_EXPECTATION"])
        switch expectation {
        case "unavailable":
            #expect(observations["resourceExportError"]?.contains("Could not read original from Photos") == true)
            #expect(observations["resourceExport"] == nil)
            #expect(observations["originalEditingInput"] == nil)
            #expect(observations["originalImageData"] == nil)
            #expect(!FileManager.default.fileExists(atPath: root.appending(path: "original.snapshot").path))
        case "readable":
            #expect(observations["resourceExport"] != nil)
        default: Issue.record("Specify an explicit readable or unavailable original expectation")
        }
    }
}
