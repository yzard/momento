import Foundation
import Testing
@testable import Momento

@Suite(.serialized) @MainActor struct TimelineTests {
    @Test func newerFilterResponseWinsOverAnOlderInFlightRequest() async throws {
        ControlledTimelineProtocol.pending.reset()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [ControlledTimelineProtocol.self]
        let api = APIClient(transport: URLSession(configuration: configuration), session: APIClientTests.session())
        let state = TimelineState()
        let first = Task { await state.load(api: api, destination: .timeline, reset: true, direction: "older") }
        while ControlledTimelineProtocol.pending.read() == nil { await Task.yield() }
        state.period = "month"
        await state.load(api: api, destination: .timeline, reset: true, direction: "older")
        ControlledTimelineProtocol.pending.read()?.finish(date: "old-day")
        await first.value
        #expect(state.groups.first?.date == "new-month")
        #expect(!state.loading)
        #expect(state.error == nil)
    }
    @Test func removesOnlyAffectedMediaAndSelection() throws {
        let data = Data("""
        {"id":1,"filename":"a.jpg","originalFilename":"a.jpg","mediaType":"image","createdAt":"2026-01-01"}
        """.utf8)
        var second = try JSONDecoder().decode(Media.self, from: data); second.id = 2
        let state = TimelineState()
        state.groups = [TimelineGroup(date: "2026-01-01", media: [try JSONDecoder().decode(Media.self, from: data), second])]
        state.selection = [1, 2]; state.search = "mountain"; state.period = "month"
        state.remove([1])
        #expect(state.media.map(\.id) == [2])
        #expect(state.selection == [2])
        #expect(state.search == "mountain")
        #expect(state.period == "month")
    }
}

private final class ControlledTimelineProtocol: URLProtocol, @unchecked Sendable {
    static let pending = TimelineRequestGate()
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let first = Self.pending.register(self)
        if !first { finish(date: "new-month") }
    }
    func finish(date: String) {
        let data = Data("""
        {"groups":[{"date":"\(date)","media":[]}],"hasOlder":false,"hasNewer":false}
        """.utf8)
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: ["Content-Type":"application/json"])!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private final class TimelineRequestGate: @unchecked Sendable {
    private let lock = NSLock()
    private var first: ControlledTimelineProtocol?
    func reset() { lock.lock(); defer { lock.unlock() }; first = nil }
    func read() -> ControlledTimelineProtocol? { lock.lock(); defer { lock.unlock() }; return first }
    func register(_ request: ControlledTimelineProtocol) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard first == nil else { return false }
        first = request
        return true
    }
}
