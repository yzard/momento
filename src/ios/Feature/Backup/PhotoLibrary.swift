import Foundation
import Photos
import UniformTypeIdentifiers
import ImageIO
import AVFoundation

enum PhotoLibrary {
    static func saveOriginal(_ file: URL, isVideo: Bool) async throws {
        try await PHPhotoLibrary.shared().performChanges { @Sendable in
            if isVideo { PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: file) }
            else { PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: file) }
        }
    }

    @concurrent static func scan(existing: [BackupRecord], supportedExtensions: [String]) async throws -> [BackupRecord] {
        let access = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        guard access == .authorized || access == .limited else { throw ClientError.message("Allow Photos access in Settings before backing up.") }
        let known = Set(existing.map(\.id))
        let knownOriginals = Set(existing.filter { $0.duplicateOriginalOf == nil }.compactMap(PhotoOriginalIdentity.init(record:)))
        let options = PHFetchOptions()
        options.includeAssetSourceTypes = [.typeUserLibrary, .typeCloudShared, .typeiTunesSynced]
        let assets = PHAsset.fetchAssets(with: options)
        var result: [BackupRecord] = []
        assets.enumerateObjects { asset, _, _ in
            let resources = PHAssetResource.assetResources(for: asset)
            for (index, resource) in resources.enumerated() where [.photo, .video, .pairedVideo, .alternatePhoto].contains(resource.type) {
                let modified = (asset.modificationDate ?? asset.creationDate ?? Date(timeIntervalSince1970: 0)).ISO8601Format()
                let identity = PhotoOriginalIdentity(assetId: asset.localIdentifier, filename: resource.originalFilename, type: resource.type.rawValue)
                let id = identity.recordId(uniformType: resource.uniformTypeIdentifier)
                guard !knownOriginals.contains(identity), !known.contains(id) else { continue }
                let ext = "." + (resource.originalFilename as NSString).pathExtension.lowercased()
                guard supportedExtensions.contains(ext) else { continue }
                var metadata: [String: JSONValue] = ["photoKit": .object([
                    "localIdentifier": .string(asset.localIdentifier), "resourceType": .number(Double(resource.type.rawValue)),
                    "uniformTypeIdentifier": .string(resource.uniformTypeIdentifier),
                    "pixelWidth": .number(Double(asset.pixelWidth)), "pixelHeight": .number(Double(asset.pixelHeight)),
                    "favorite": .bool(asset.isFavorite), "duration": .number(asset.duration),
                    "isScreenshot": .bool(asset.mediaSubtypes.contains(.photoScreenshot))
                ])]
                if let date = asset.creationDate { metadata["photoTakenTime"] = .object(["timestamp": .string(String(Int64(date.timeIntervalSince1970)))]) }
                if let location = asset.location {
                    metadata["geoData"] = .object(["latitude": .number(location.coordinate.latitude), "longitude": .number(location.coordinate.longitude), "altitude": .number(location.altitude)])
                }
                result.append(BackupRecord(id: id, assetId: asset.localIdentifier, resourceIndex: index, filename: resource.originalFilename,
                    mimeType: UTType(resource.uniformTypeIdentifier)?.preferredMIMEType ?? (asset.mediaType == .video ? "video/quicktime" : "image/jpeg"),
                    modifiedAt: modified, operationId: UUID().uuidString, uploadId: nil, state: .queued,
                    byteSize: 0, uploadedSize: 0, contentHash: nil, serverHash: nil, metadata: metadata, error: nil, cameraCapture: nil))
            }
        }
        return result
    }

    static func cameraOriginal(_ url: URL, mimeType: String) async throws -> Bool {
        if mimeType.hasPrefix("image/") {
            guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
                  let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
                  let tiff = properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any] else { return false }
            return tiff[kCGImagePropertyTIFFMake] as? String != nil && tiff[kCGImagePropertyTIFFModel] as? String != nil
        }
        // Durable snapshots use .original, so AVFoundation cannot infer the movie container from the path.
        let asset = AVURLAsset(url: url, options: [AVURLAssetOverrideMIMETypeKey: mimeType])
        let metadata = try await asset.loadMetadata(for: .quickTimeMetadata)
        for value in metadata where value.identifier == .quickTimeMetadataMake || value.identifier == .quickTimeMetadataModel {
            if let text = try await value.load(.stringValue), !text.isEmpty { return true }
        }
        return false
    }

    static func export(_ record: BackupRecord, to destination: URL, cellular: Bool) async throws {
        if FileManager.default.fileExists(atPath: destination.path) { return }
        guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [record.assetId], options: nil).firstObject else {
            throw ClientError.message("Original is no longer accessible. Check Photos permissions or iCloud availability.")
        }
        let resources = PHAssetResource.assetResources(for: asset)
        let index = try PhotoOriginalResource.select(for: record, from: resources.map {
            PhotoOriginalResource(type: $0.type.rawValue, filename: $0.originalFilename, uniformType: $0.uniformTypeIdentifier)
        })
        let options = PHAssetResourceRequestOptions()
        options.isNetworkAccessAllowed = true
        try await PhotoLibraryOriginalExport.write(to: destination, attempts: 2) { pending in
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                PHAssetResourceManager.default().writeData(for: resources[index], toFile: pending, options: options) { error in
                    if let error { continuation.resume(throwing: error) } else { continuation.resume() }
                }
            }
        }
    }
}
