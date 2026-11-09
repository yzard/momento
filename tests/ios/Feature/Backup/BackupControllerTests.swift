import Foundation
import Synchronization
import Testing
@testable import Momento

@Suite(.serialized) @MainActor struct BackupControllerTests {
    @Test func idleCancellationLeavesFailedHistoryAvailableForRetry() async throws {
        PausedBackupProtocol.state.withLock { $0 = .init() }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [PausedBackupProtocol.self]
        let api = APIClient(transport: URLSession(configuration: config), session: APIClientTests.session())
        let root = FileManager.default.temporaryDirectory.appending(path: "idle-cancel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let backup = try BackupController(api: api, root: root)
        var failed = BackupTests().record()
        failed.state = .failed
        failed.error = "Cannot Open"
        try await backup.store.save(failed)
        try await backup.reload()
        backup.cancel()
        #expect(!backup.cancelling)
        #expect(!backup.running)
        let retained = try #require(try await backup.store.records().first)
        #expect(retained.state == .failed)
        #expect(retained.operationId == failed.operationId)
        #expect(retained.error == failed.error)
        #expect(PausedBackupProtocol.state.withLock { $0.count } == 0)
        await backup.stop()
    }

    @Test func cancellingAnActiveBatchStopsItsNetworkRequestAndPreservesIntent() async throws {
        PausedBackupProtocol.state.withLock { $0 = .init() }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [PausedBackupProtocol.self]
        let api = APIClient(transport: URLSession(configuration: config), session: APIClientTests.session())
        let root = FileManager.default.temporaryDirectory.appending(path: "batch-cancel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let backup = try BackupController(api: api, root: root)
        try await backup.store.save(BackupTests().record())
        let file = root.appending(path: "request.batch-body")
        try Data("protected-original".utf8).write(to: file)
        let work = Task<BackupBatchResponse, Error> {
            let bytes = try await api.uploadFile(path: "backup/upload/batch", file: file, length: 18, cellular: true, progress: { _ in })
            return try JSONDecoder().decode(BackupBatchResponse.self, from: bytes)
        }
        backup.batchTransfer = work; backup.running = true
        let deadline = Date().addingTimeInterval(5)
        while PausedBackupProtocol.state.withLock({ $0.request == nil }) && Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        try #require(PausedBackupProtocol.state.withLock { $0.request != nil })
        backup.cancel()
        await #expect(throws: (any Error).self) { try await work.value }
        let cancelled = Date().addingTimeInterval(2)
        while Date() < cancelled {
            let state = try await backup.store.records().first?.state
            if PausedBackupProtocol.state.withLock({ $0.cancelled }) && state == .cancelling { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(PausedBackupProtocol.state.withLock { $0.cancelled })
        #expect(try await backup.store.records().first?.state == .cancelling)
        backup.running = false
        await backup.stop()
    }

    @Test func logoutStopsScheduledBackupBeforeTheSessionCanChange() async throws {
        PausedBackupProtocol.state.withLock { $0 = .init() }
        let defaults = UserDefaults.standard
        let previousSchedule = defaults.object(forKey: "backupNextScheduled")
        defer {
            if let previousSchedule { defaults.set(previousSchedule, forKey: "backupNextScheduled") }
            else { defaults.removeObject(forKey: "backupNextScheduled") }
        }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [PausedBackupProtocol.self]
        let api = APIClient(transport: URLSession(configuration: config), session: APIClientTests.session())
        let root = FileManager.default.temporaryDirectory.appending(path: "background-stop-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let backup = try BackupController(api: api, root: root)
        var record = BackupTests().record()
        record.state = .cancelling
        try await backup.store.save(record)
        let background = Task { await backup.runScheduled() }
        let deadline = Date().addingTimeInterval(10)
        while PausedBackupProtocol.state.withLock({ $0.request == nil }) && Date() < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        let request = PausedBackupProtocol.state.withLock { $0.request }
        #expect(request != nil, "The scheduled task must reach the deliberately paused request")
        await backup.stop()
        #expect(!backup.running, "Logout must wait for the background worker to stop")
        let cancellationDeadline = Date().addingTimeInterval(1)
        while !PausedBackupProtocol.state.withLock({ $0.cancelled }) && Date() < cancellationDeadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(PausedBackupProtocol.state.withLock { $0.cancelled })
        // Release an old implementation's request as well, so a failing test cannot hang.
        request?.finish()
        await background.value
        backup.start()
        await backup.runScheduled()
        #expect(!backup.running)
        #expect(backup.nextScheduled == nil)
        #expect(PausedBackupProtocol.state.withLock { $0.count } == 1)
        let newAccountSchedule = Date().addingTimeInterval(3600)
        defaults.set(newAccountSchedule, forKey: "backupNextScheduled")
        backup.schedule()
        #expect(defaults.object(forKey: "backupNextScheduled") as? Date == newAccountSchedule,
                "A stopped account's delayed completion must not clear the new account's schedule")
    }
}

private final class PausedBackupProtocol: URLProtocol, @unchecked Sendable {
    struct State { var request: PausedBackupProtocol?; var cancelled = false; var count = 0 }
    static let state = Mutex(State())
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() { Self.state.withLock { $0.request = self; $0.count += 1 } }
    override func stopLoading() { Self.state.withLock { $0.cancelled = true } }
    func finish() {
        guard !Self.state.withLock({ $0.cancelled }) else { return }
        let bytes = Data("{\"uploadId\":\"upload\",\"status\":\"cancelled\",\"uploadedSize\":20,\"expectedSize\":100,\"contentHash\":\"abc\"}".utf8)
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: ["Content-Type": "application/json"])!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: bytes)
        client?.urlProtocolDidFinishLoading(self)
    }
}
