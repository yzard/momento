import Foundation
import Testing
@testable import Momento

struct BackupBatchTests {
    let limits = BackupBatchCapabilities(version: 1, maxBytes: 50_000_000, maxItems: 256, maxManifestBytes: 1_048_576)

    @Test func groupsByOriginalBytesAndKeepsOversizedFilesAlone() {
        #expect(backupBatchCanAppend(sizes: [], next: 70_000_000, limits: limits))
        #expect(!backupBatchCanAppend(sizes: [70_000_000], next: 1, limits: limits))
        #expect(backupBatchCanAppend(sizes: [20_000_000], next: 30_000_000, limits: limits))
        #expect(!backupBatchCanAppend(sizes: [20_000_000], next: 30_000_001, limits: limits))
        #expect(!backupBatchCanAppend(sizes: [], next: 0, limits: limits))
        #expect(!backupBatchCanAppend(sizes: Array(repeating: 1, count: 256), next: 1, limits: limits))
        #expect(!backupBatchCanAppend(sizes: [Int64.max], next: Int64.max, limits: limits))
    }

    @Test func writesLengthFramedOriginalsAndDetectsChangedSnapshots() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "batch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let first = Data("original-one".utf8), second = Data("original-two".utf8)
        let files = try [first, second].enumerated().map { index, bytes in
            let url = root.appending(path: "original-\(index)"); try bytes.write(to: url)
            return BackupBatchFile(recordId: "\(index)", filename: "\(index).jpg", file: url, uploadId: "upload-\(index)", byteSize: Int64(bytes.count), contentHash: SHA256Key.digest(bytes))
        }
        let output = root.appending(path: "payload")
        let payload = try await BackupBatchPayload.write(originals: files, to: output, limits: limits)
        let raw = try Data(contentsOf: output)
        #expect(raw.count == payload.length)
        let headerLength = raw.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
        let manifest = try JSONDecoder().decode(BackupBatchManifest.self, from: raw.subdata(in: 4..<4+headerLength))
        #expect(manifest.entries.map(\.uploadId) == ["upload-0", "upload-1"])
        #expect(raw.dropFirst(4 + headerLength) == first + second)
        #expect(payload.fileProgress(sent: payload.headerBytes + Int64(first.count) + 2)?.filename == "1.jpg")
        #expect(payload.fileProgress(sent: payload.headerBytes + Int64(first.count) + 2)?.finished == 2)
        try Data("mutated".utf8).write(to: files[1].file)
        let failed = root.appending(path: "incomplete")
        await #expect(throws: (any Error).self) { try await BackupBatchPayload.write(originals: files, to: failed, limits: limits) }
        #expect(!FileManager.default.fileExists(atPath: failed.path))
    }

    @Test func rejectsMissingDuplicateAndAmbiguousAcknowledgements() {
        let upload = BackupTests().response("uploading", offset: 0, hash: "abc")
        for response in [BackupBatchResponse(results: []),
            BackupBatchResponse(results: [.init(key: "one", upload: upload, error: nil), .init(key: "one", upload: upload, error: nil)]),
            BackupBatchResponse(results: [.init(key: "one", upload: nil, error: nil)]),
            BackupBatchResponse(results: [.init(key: "one", upload: upload, error: .init(status: 400, detail: "invalid"))])] {
            #expect(throws: (any Error).self) { try response.indexed(keys: ["one"]) }
        }
    }

    @Test @MainActor func uploadsTwoOriginalsInOneFileBackedRequest() async throws {
        guard let originText = ProcessInfo.processInfo.environment["MOMENTO_UI_TEST_ORIGIN"], originText.hasPrefix("http://127.0.0.1:"), let origin = URL(string: originText) else { return }
        let root = FileManager.default.temporaryDirectory.appending(path: "batch-network-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        var session = APIClientTests.session(); session.origin = origin; session.tokens.accessToken = "fixture-access"
        let api = APIClient(transport: URLSession(configuration: .ephemeral), session: session)
        let backup = try BackupController(api: api, root: root)
        let caps = try await api.capabilities()
        var group: [BackupRecord] = []
        for index in 0..<2 {
            var record = BackupTests().record()
            record.id = "batch-\(index)"; record.operationId = UUID().uuidString; record.uploadId = nil; record.state = .queued
            let bytes = Data(repeating: UInt8(index + 1), count: 200_000 + index)
            record.byteSize = Int64(bytes.count); record.contentHash = SHA256Key.digest(bytes); record.uploadedSize = 0
            try bytes.write(to: await backup.store.snapshot(record))
            try await backup.store.save(record); group.append(record)
        }
        try await backup.reload()
        try await backup.flushBatch(group, device: "batch-test-device", capabilities: caps.backup, limits: limits)
        #expect(backup.records.count == 2)
        #expect(backup.records.allSatisfy { $0.state == .completed && $0.contentHash == $0.serverHash && $0.uploadedSize == $0.byteSize })
        #expect(try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil).allSatisfy { $0.pathExtension == "json" })
        await backup.stop()
    }
}
