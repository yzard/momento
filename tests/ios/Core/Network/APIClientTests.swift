import Foundation
import Synchronization
import Testing
@testable import Momento

final class StubProtocol: URLProtocol, @unchecked Sendable {
    struct State {
        var requests: [URLRequest] = []
        var replies: [(Int, Data)] = []
        var headers: [String: String] = [:]
    }
    static let state = Mutex(State())
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let reply = Self.state.withLock { state in
            state.requests.append(request)
            return state.replies.isEmpty ? (500, Data()) : state.replies.removeFirst()
        }
        var headers = Self.state.withLock { $0.headers }
        if headers["Content-Type"] == nil { headers["Content-Type"] = "application/json" }
        let response = HTTPURLResponse(url: request.url!, statusCode: reply.0, httpVersion: nil, headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.1)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

@Suite(.serialized) struct APIClientTests {
    static func session() -> SavedSession {
        SavedSession(origin: URL(string: "https://example.invalid")!, tokens: TokenPair(accessToken: "old", refreshToken: "refresh", tokenType: "bearer"),
            user: User(id: 7, username: "test", email: "test@example.invalid", role: "user", isReserved: false, mustChangePassword: false, isActive: true, createdAt: "2026-01-01"), namespace: "test-account")
    }
    private func client(_ replies: [(Int, String)]) -> APIClient {
        StubProtocol.state.withLock { $0 = StubProtocol.State(replies: replies.map { ($0.0, Data($0.1.utf8)) }) }
        let config = URLSessionConfiguration.ephemeral; config.protocolClasses = [StubProtocol.self]
        return APIClient(transport: URLSession(configuration: config), session: Self.session())
    }

    @Test(arguments: ["https://host/path", "https://user:password@host", "https://host?q=1", "https://host/#fragment", "ftp://host", "http://host"])
    func rejectsInvalidReleaseOrigins(_ value: String) {
        #expect(throws: (any Error).self) { try APIClient.origin(value, allowHTTP: false) }
    }
    @Test func acceptsLocalDebugOrigin() throws {
        #expect(try APIClient.origin(" http://192.168.1.2:8080/ ", allowHTTP: true).port == 8080)
    }
    @Test func normalizesServerIdentityForBackupIsolation() throws {
        #expect(try APIClient.origin("https://EXAMPLE.com:443/", allowHTTP: false) == APIClient.origin("https://example.com", allowHTTP: false))
    }
    @Test func sendsBearerAndTypedJSON() async throws {
        let api = client([(200, "{\"message\":\"ok\"}")])
        let result: MessageResponse = try await api.call("media/delete", MediaIdsRequest(mediaIds: [2, 3]))
        #expect(result.message == "ok")
        let request = try #require(StubProtocol.state.withLock { $0.requests.first })
        #expect(request.httpMethod == "POST")
        #expect(request.url?.path == "/api/v1/media/delete")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer old")
        #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
    }
    @Test func refreshesAndReplaysOnlyOnce() async throws {
        let api = client([(401, "{\"detail\":\"expired\"}"), (200, "{\"accessToken\":\"new\",\"refreshToken\":\"rotated\",\"tokenType\":\"bearer\"}"), (200, "{\"message\":\"ok\"}")])
        let _: MessageResponse = try await api.call("user/get", EmptyRequest())
        let requests = StubProtocol.state.withLock { $0.requests }
        #expect(requests.count == 3)
        #expect(requests[1].url?.path == "/api/v1/user/refresh")
        #expect(requests[1].value(forHTTPHeaderField: "Authorization") == nil)
        #expect(requests[2].value(forHTTPHeaderField: "Authorization") == "Bearer new")
        try await api.forgetSession()
    }
    @Test func exposesServerDetail() async {
        let api = client([(403, "{\"detail\":\"Administrator required\"}")])
        do { let _: MessageResponse = try await api.call("ai/start", EmptyRequest()); Issue.record("Expected rejection") }
        catch { #expect(error.localizedDescription == "Administrator required") }
    }
    @Test func chunkUsesByteRangeHashAndMobilePolicy() async throws {
        let api = client([(200, "{}")])
        let _: Data = try await api.bytes(path: "backup/upload/chunk/id", method: "PUT", body: Data([1, 2]), contentType: "application/octet-stream",
            headers: ["Content-Range": "bytes 4-5/10", "X-Content-SHA256": "abc"], cellular: false)
        let request = try #require(StubProtocol.state.withLock { $0.requests.first })
        #expect(request.httpMethod == "PUT")
        #expect(request.value(forHTTPHeaderField: "Content-Range") == "bytes 4-5/10")
        #expect(request.value(forHTTPHeaderField: "X-Content-SHA256") == "abc")
        #expect(!request.allowsCellularAccess)
    }

    @Test func lostChunkAcknowledgementReconcilesWithoutResending() async throws {
        let response = "{\"uploadId\":\"u\",\"status\":\"uploading\",\"uploadedSize\":4,\"expectedSize\":8,\"contentHash\":\"original\"}"
        let api = client([(503, "{\"detail\":\"response lost\"}"), (200, response)])
        let result = try await BackupChunkTransfer.upload(api: api, uploadId: "u", offset: 0, total: 8, originalHash: "original", bytes: Data([1,2,3,4]), cellular: false, attempts: 2)
        #expect(result.uploadedSize == 4)
        #expect(StubProtocol.state.withLock { $0.requests.map(\.httpMethod) } == ["PUT", "POST"])
    }
    @Test func reconciliationRejectsWrongOriginal() async {
        let response = "{\"uploadId\":\"u\",\"status\":\"uploading\",\"uploadedSize\":4,\"expectedSize\":8,\"contentHash\":\"different\"}"
        let api = client([(503, "{\"detail\":\"lost\"}"), (200, response)])
        await #expect(throws: (any Error).self) {
            try await BackupChunkTransfer.upload(api: api, uploadId: "u", offset: 0, total: 8, originalHash: "original", bytes: Data([1,2,3,4]), cellular: false, attempts: 2)
        }
        #expect(StubProtocol.state.withLock { $0.requests.count } == 2)
    }
    @Test func authenticatedVideoRangesPreserveBounds() async throws {
        let api = client([(206, "abcd")])
        StubProtocol.state.withLock { $0.headers = ["Content-Range": "bytes 4-7/16", "Content-Type": "video/mp4"] }
        let range = try await api.mediaRange(path: "media/1/original", offset: 4, length: 4)
        #expect(range.total == 16)
        #expect(range.data == Data("abcd".utf8))
        #expect(range.mimeType == "video/mp4")
        #expect(StubProtocol.state.withLock { $0.requests.first?.value(forHTTPHeaderField: "Range") } == "bytes=4-7")
    }
    @Test func refusesUnboundedOrMismatchedVideoResponse() async {
        let api = client([(200, "full-file")])
        await #expect(throws: (any Error).self) { try await api.mediaRange(path: "media/1/original", offset: 0, length: 4) }
    }
}
