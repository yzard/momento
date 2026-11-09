import Foundation
import Photos
import Synchronization
import Testing
@testable import Momento

@Suite(.serialized) @MainActor struct PhotoKitBackupTests {
    @Test(arguments: [("camera-fixture", true), ("noncamera-fixture", false)])
    func readsMovieCameraMetadataFromAnOpaqueSnapshot(_ fixture: String, _ expected: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "movie-metadata-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = try #require(Bundle(for: PhotoKitFixtureBundle.self).url(forResource: fixture, withExtension: "mov"))
        let snapshot = root.appending(path: "operation.original")
        try FileManager.default.copyItem(at: source, to: snapshot)
        let originalHash = try await SHA256Key.file(snapshot)
        #expect(try await PhotoLibrary.cameraOriginal(snapshot, mimeType: "video/quicktime") == expected)
        #expect(try await SHA256Key.file(snapshot).0 == originalHash.0)
    }

    @Test func corruptMoviesRemainFailuresInsteadOfBeingSilentlyExcluded() async throws {
        let file = FileManager.default.temporaryDirectory.appending(path: "invalid-movie-\(UUID().uuidString).original")
        defer { try? FileManager.default.removeItem(at: file) }
        try Data("Not a QuickTime movie".utf8).write(to: file)
        await #expect(throws: (any Error).self) { try await PhotoLibrary.cameraOriginal(file, mimeType: "video/quicktime") }
    }

    @Test(.enabled(if: ProcessInfo.processInfo.environment["MOMENTO_PHOTOKIT_FIXTURE"] == "1"))
    func exportsAndVerifiesAnUnmodifiedPhotoKitOriginal() async throws {
        #if targetEnvironment(simulator)
        let originText = try #require(ProcessInfo.processInfo.environment["MOMENTO_UI_TEST_ORIGIN"])
        try #require(originText.hasPrefix("http://127.0.0.1:"))
        let origin = try #require(URL(string: originText))
        let authorization = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        try #require(authorization == .authorized, "Photos permission for \(Bundle.main.bundleIdentifier ?? "unknown host")")
        let (bytes, _) = try await URLSession.shared.data(from: origin.appending(path: "fixture-original"))
        let identifier = Mutex<String?>(nil)
        try await PHPhotoLibrary.shared().performChanges { @Sendable in
            let request = PHAssetCreationRequest.forAsset()
            let options = PHAssetResourceCreationOptions()
            options.originalFilename = "momento-backup-fixture.png"
            request.addResource(with: .photo, data: bytes, options: options)
            identifier.withLock { $0 = request.placeholderForCreatedAsset?.localIdentifier }
        }
        let assetId = try #require(identifier.withLock { $0 })
        let root = FileManager.default.temporaryDirectory.appending(path: "photokit-test-\(UUID().uuidString)")
        let defaults = UserDefaults.standard
        let keys = ["backupCameraOnly", "backupCellular", "backupDaily", "backupDevice", "backupNextScheduled"]
        let previous = keys.map { ($0, defaults.object(forKey: $0)) }
        defer {
            try? FileManager.default.removeItem(at: root)
            for (key, value) in previous { if let value { defaults.set(value, forKey: key) } else { defaults.removeObject(forKey: key) } }
        }
        do {
            var session = APIClientTests.session()
            session.origin = origin; session.tokens.accessToken = "fixture-access"
            let api = APIClient(transport: URLSession(configuration: .ephemeral), session: session)
            let backup = try BackupController(api: api, root: root)
            backup.cameraOnly = false
            backup.daily = false
            await backup.run(requestPermission: false)
            let record = try #require(backup.records.first { $0.assetId == assetId })
            #expect(record.state == .completed)
            #expect(record.byteSize == bytes.count)
            #expect(record.contentHash == SHA256Key.digest(bytes))
            #expect(record.serverHash == SHA256Key.digest(bytes))
            #expect(backup.latestError == nil)
            await backup.stop()
            // Favorite/sync metadata and resource ordering are not original-content versions.
            try await PHPhotoLibrary.shared().performChanges { @Sendable in
                if let asset = PHAsset.fetchAssets(withLocalIdentifiers: [assetId], options: nil).firstObject {
                    PHAssetChangeRequest(for: asset).isFavorite = !asset.isFavorite
                }
            }
            var metadataChanged = record
            metadataChanged.modifiedAt = "2000-01-01T00:00:00Z"
            metadataChanged.resourceIndex = 999
            let unchanged = root.appending(path: "same-unedited-original.png")
            try await PhotoLibrary.export(metadataChanged, to: unchanged, cellular: false)
            #expect(try Data(contentsOf: unchanged) == bytes)
            let rescan = try await PhotoLibrary.scan(existing: [metadataChanged], supportedExtensions: [".png"])
            #expect(!rescan.contains { $0.assetId == assetId })
            // Reopening the durable store and rescanning must not upload an unchanged original again.
            let reopened = try BackupController(api: api, root: root)
            await reopened.run(requestPermission: false)
            let retained = try #require(reopened.records.first { $0.assetId == assetId })
            #expect(retained.uploadId == record.uploadId)
            #expect(retained.operationId == record.operationId)
            #expect(retained.state == .completed)
            #expect(reopened.records.filter { $0.assetId == assetId }.count == 1)
            #expect(reopened.latestError == nil)
            await reopened.stop()
            // Exercise the viewer's actual save-to-Photos callback, then export its bytes.
            let downloaded = root.appending(path: "viewer-save-\(UUID().uuidString).png")
            try bytes.write(to: downloaded)
            try await PhotoLibrary.saveOriginal(downloaded, isVideo: false)
            let scanned = try await PhotoLibrary.scan(existing: [], supportedExtensions: [".png"])
            let saved = try #require(scanned.first { $0.filename == downloaded.lastPathComponent })
            let exported = root.appending(path: "viewer-export.png")
            try await PhotoLibrary.export(saved, to: exported, cellular: false)
            #expect(try Data(contentsOf: exported) == bytes)
            try await verifiesFailedMOVRetry(api: api, root: root.appending(path: "mov-retry"))
        } catch {
            throw error
        }
        #else
        Issue.record("PhotoKit fixtures must never run on a physical device")
        #endif
    }

    #if targetEnvironment(simulator)
    private func verifiesFailedMOVRetry(api: APIClient, root: URL) async throws {
        let movie = try #require(Bundle(for: PhotoKitFixtureBundle.self).url(forResource: "camera-fixture", withExtension: "mov"))
        let bytes = try Data(contentsOf: movie)
        let identifier = Mutex<String?>(nil)
        try await PHPhotoLibrary.shared().performChanges { @Sendable in
            let request = PHAssetCreationRequest.forAsset()
            let options = PHAssetResourceCreationOptions()
            options.originalFilename = "momento-camera-fixture.MOV"
            options.uniformTypeIdentifier = "com.apple.quicktime-movie"
            request.addResource(with: .video, fileURL: movie, options: options)
            identifier.withLock { $0 = request.placeholderForCreatedAsset?.localIdentifier }
        }
        let assetId = try #require(identifier.withLock { $0 })
        let scanned = try await PhotoLibrary.scan(existing: [], supportedExtensions: [".mov"])
        var failed = try #require(scanned.first { $0.assetId == assetId })
        failed.state = .failed
        failed.error = "Cannot Open"
        let backup = try BackupController(api: api, root: root)
        backup.cameraOnly = true
        backup.daily = false
        try await backup.store.save(failed)
        let snapshot = await backup.store.snapshot(failed)
        try bytes.write(to: snapshot)
        await backup.run(requestPermission: false)
        let completed = try #require(backup.records.first { $0.id == failed.id })
        #expect(completed.state == .completed, "MOV retry failed: \(completed.error ?? backup.latestError ?? "unknown error")")
        #expect(completed.operationId == failed.operationId)
        #expect(completed.cameraCapture == true)
        #expect(completed.contentHash == SHA256Key.digest(bytes))
        #expect(completed.serverHash == completed.contentHash)
        #expect(completed.byteSize == bytes.count)
        #expect(completed.error == nil)
        #expect(backup.latestError == nil)
        await backup.stop()
        let reopened = try BackupController(api: api, root: root)
        await reopened.run(requestPermission: false)
        let retained = try #require(reopened.records.first { $0.id == failed.id })
        #expect(retained.state == .completed)
        #expect(retained.uploadId == completed.uploadId)
        #expect(retained.operationId == completed.operationId)
        await reopened.stop()
    }
    #endif
}

private final class PhotoKitFixtureBundle: NSObject {}
