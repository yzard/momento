import Testing
@testable import Momento

@MainActor struct AppModelTests {
    @Test func remainsBusyUntilEveryConcurrentOperationFinishes() async {
        let model = AppModel()
        let firstGate = OperationGate()
        let secondGate = OperationGate()
        let first = Task { await model.perform { await firstGate.wait() } }
        while !(await firstGate.waiting) { await Task.yield() }
        let second = Task { await model.perform { await secondGate.wait() } }
        while !(await secondGate.waiting) { await Task.yield() }
        #expect(model.busy)
        await firstGate.release()
        await first.value
        #expect(model.busy)
        await secondGate.release()
        await second.value
        #expect(!model.busy)
    }
}

private actor OperationGate {
    private var continuation: CheckedContinuation<Void, Never>?
    var waiting: Bool { continuation != nil }
    func wait() async { await withCheckedContinuation { continuation = $0 } }
    func release() { continuation?.resume(); continuation = nil }
}
