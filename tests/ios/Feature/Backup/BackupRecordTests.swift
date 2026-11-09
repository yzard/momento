import Foundation
import Testing
@testable import Momento

struct BackupTests {
    func record() -> BackupRecord {
        BackupRecord(id: "test", assetId: "asset", resourceIndex: 0, filename: "original.jpg", mimeType: "image/jpeg", modifiedAt: "2026-01-01T00:00:00Z", operationId: "operation",
            uploadId: "upload", state: .uploading, byteSize: 100, uploadedSize: 20, contentHash: "abc", serverHash: nil, metadata: [:], error: nil, cameraCapture: true)
    }
    func response(_ state: String, offset: Int64, hash: String?) -> BackupUploadResponse {
        BackupUploadResponse(uploadId: "upload", status: state, uploadedSize: offset, expectedSize: 100, contentHash: hash, mediaId: nil, error: nil)
    }
    @Test func completionRequiresMatchingHashAndAllBytes() throws {
        var item = record()
        #expect(throws: (any Error).self) { try item.accept(response("completed", offset: 100, hash: nil)) }
        #expect(throws: (any Error).self) { try item.accept(response("completed", offset: 100, hash: "wrong")) }
        #expect(throws: (any Error).self) { try item.accept(response("completed", offset: 99, hash: "abc")) }
        try item.accept(response("completed", offset: 100, hash: "abc"))
        #expect(item.state == .completed)
        #expect(item.serverHash == item.contentHash)
    }
    @Test(arguments: [Int64(-1), 101]) func rejectsInvalidOffset(_ offset: Int64) {
        var item = record()
        #expect(throws: (any Error).self) { try item.accept(response("uploading", offset: offset, hash: "abc")) }
    }
    @Test(arguments: ["queued", "processing"]) func serverQueueIsNotCompletion(_ status: String) throws {
        var item = record(); try item.accept(response(status, offset: 100, hash: "abc"))
        #expect(item.state == .serverProcessing)
        #expect(item.state.active)
    }
    @Test func unknownStateFailsClosed() {
        var item = record()
        #expect(throws: (any Error).self) { try item.accept(response("invented", offset: 10, hash: "abc")) }
        #expect(item.uploadedSize == 20)
        #expect(item.serverHash == nil)
        #expect(item.state == .uploading)
    }
    @Test(arguments: ["", "another-upload"], ["completed", "expired", "cancelled"]) func rejectsUnrelatedUpload(_ identifier: String, _ status: String) {
        var item = record()
        let result = BackupUploadResponse(uploadId: identifier, status: status, uploadedSize: 100,
            expectedSize: 100, contentHash: "abc", mediaId: nil, error: nil)
        #expect(throws: (any Error).self) { try item.accept(result) }
        #expect(item.uploadId == "upload")
        #expect(item.uploadedSize == 20)
        #expect(item.state == .uploading)
    }
    @Test func hashesOriginalBytes() {
        #expect(SHA256Key.digest(Data("abc".utf8)) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    }
    @Test func durableCancellationSurvivesLateChunkAcknowledgement() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = try BackupStore(root: root)
        var item = record(); try await store.save(item)
        try await store.requestCancellation()
        item.uploadedSize = 40; try await store.save(item)
        let reopened = try BackupStore(root: root)
        let saved = try #require(try await reopened.records().first)
        #expect(saved.state == .cancelling)
        #expect(saved.uploadedSize == 40)
        item.state = .completed; item.serverHash = "abc"; try await reopened.save(item)
        #expect(try await reopened.records().first?.state == .completed)
    }
    @Test func cannotClearActiveHistory() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = try BackupStore(root: root); var item = record(); try await store.save(item)
        await #expect(throws: (any Error).self) { try await store.clear() }
        item.state = .cancelled; try await store.save(item); try await store.clear()
        #expect(try await store.records().isEmpty)
    }
    @Test func preservesResourceMetadata() throws {
        var item = record(); item.metadata = ["photoKit": .object(["resourceType": .number(9), "localIdentifier": .string("live-photo")]), "geoData": .object(["latitude": .number(42.3)])]
        let decoded = try JSONDecoder().decode(BackupRecord.self, from: JSONEncoder().encode(item))
        #expect(decoded.metadata == item.metadata)
    }
}
