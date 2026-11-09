import Foundation
import Photos
import Testing
@testable import Momento

@MainActor struct PhotoLibraryOriginalExportTests {
    private func directory() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appending(path: "original-export-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    @Test func transientPhotosFailureRetriesTheSameOriginalWithoutKeepingPartialBytes() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "original.snapshot")
        let expected = Data("unmodified original bytes".utf8)
        var calls = 0
        try await PhotoLibraryOriginalExport.write(to: file, attempts: 2) { pending in
            calls += 1
            #expect(!FileManager.default.fileExists(atPath: pending.path))
            if calls == 1 {
                try Data("partial".utf8).write(to: pending)
                throw NSError(domain: PHPhotosErrorDomain, code: -1)
            }
            try expected.write(to: pending)
        }
        #expect(calls == 2)
        #expect(try Data(contentsOf: file) == expected)
        #expect(!FileManager.default.fileExists(atPath: file.appendingPathExtension("pending").path))
    }

    @Test func unavailableOriginalStaysAFailureAndRetainsItsUnderlyingCause() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "original.snapshot")
        var calls = 0
        do {
            try await PhotoLibraryOriginalExport.write(to: file, attempts: 2) { pending in
                calls += 1
                try Data("partial".utf8).write(to: pending)
                throw NSError(domain: PHPhotosErrorDomain, code: -1,
                    userInfo: [NSUnderlyingErrorKey: NSError(domain: "PLResourceDataStoreErrorDomain", code: 12)])
            }
            Issue.record("An unavailable original must never be marked as exported")
        } catch let error as PhotoLibraryOriginalReadError {
            #expect(error.localizedDescription.contains("unedited original"))
            #expect(error.localizedDescription.contains("PHPhotosErrorDomain -1"))
            #expect(error.localizedDescription.contains("PLResourceDataStoreErrorDomain 12"))
        }
        #expect(calls == 2)
        #expect(!FileManager.default.fileExists(atPath: file.path))
        #expect(!FileManager.default.fileExists(atPath: file.appendingPathExtension("pending").path))
    }

    @Test func cancellationDoesNotRetryOrPublishPartialBytes() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "original.snapshot")
        var calls = 0
        await #expect(throws: CancellationError.self) {
            try await PhotoLibraryOriginalExport.write(to: file, attempts: 2) { pending in
                calls += 1; try Data("partial".utf8).write(to: pending)
                throw CancellationError()
            }
        }
        #expect(calls == 1)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }

    @Test func diskErrorsDoNotRetryAndPreserveTheActualReason() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        var calls = 0
        do {
            try await PhotoLibraryOriginalExport.write(to: root.appending(path: "original.snapshot"), attempts: 2) { _ in
                calls += 1
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteOutOfSpaceError,
                    userInfo: [NSLocalizedDescriptionKey: "Not enough free space"])
            }
            Issue.record("A disk failure must remain a failure")
        } catch let error as PhotoLibraryOriginalReadError {
            #expect(error.localizedDescription.contains("Not enough free space"))
            #expect(!error.localizedDescription.contains("Download or recover"))
        }
        #expect(calls == 1)
    }

    @Test func transportFailuresKeepTheTypeThatPausesTheBackupRun() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        var calls = 0
        do {
            try await PhotoLibraryOriginalExport.write(to: root.appending(path: "original.snapshot"), attempts: 2) { _ in
                calls += 1; throw URLError(.networkConnectionLost)
            }
            Issue.record("A disconnected transport must remain retryable by the backup controller")
        } catch let error as URLError { #expect(error.code == .networkConnectionLost) }
        #expect(calls == 1)
    }

    @Test func emptyExportsAreRejectedAndSavedSnapshotsArePreserved() async throws {
        let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "original.snapshot")
        await #expect(throws: PhotoLibraryOriginalReadError.self) {
            try await PhotoLibraryOriginalExport.write(to: file, attempts: 2) { try Data().write(to: $0) }
        }
        #expect(!FileManager.default.fileExists(atPath: file.path))
        let original = Data("previously protected original".utf8)
        try original.write(to: file)
        try await PhotoLibraryOriginalExport.write(to: file, attempts: 2) { _ in
            Issue.record("A saved original must not be exported again")
        }
        #expect(try Data(contentsOf: file) == original)
    }
}
