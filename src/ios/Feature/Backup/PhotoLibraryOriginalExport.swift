import Foundation
import Photos

struct PhotoLibraryOriginalReadError: LocalizedError {
    let message: String
    var errorDescription: String? { message }

    init(_ error: any Error) {
        let native = error as NSError
        let explanation = native.domain == PHPhotosErrorDomain && native.code == -1
            ? "Photos could not read or download the unedited original. Download or recover its original in Photos, then retry."
            : native.localizedDescription
        var causes: [String] = []
        var current: NSError? = native
        var seen: Set<ObjectIdentifier> = []
        while let cause = current, causes.count < 4, seen.insert(ObjectIdentifier(cause)).inserted {
            var detail = "\(cause.domain) \(cause.code)"
            if let reason = cause.localizedFailureReason, !reason.isEmpty { detail += ": \(reason)" }
            causes.append(detail)
            current = cause.userInfo[NSUnderlyingErrorKey] as? NSError
        }
        message = "Could not read original from Photos: \(explanation) [\(causes.joined(separator: " → "))]"
    }
}

enum PhotoLibraryOriginalExport {
    static func write(to destination: URL, attempts: Int, writer: (URL) async throws -> Void) async throws {
        guard attempts > 0 else { throw ClientError.message("Original export requires at least one attempt.") }
        if FileManager.default.fileExists(atPath: destination.path) { return }
        let pending = destination.appendingPathExtension("pending")
        defer { try? FileManager.default.removeItem(at: pending) }
        for attempt in 0..<attempts {
            try Task.checkCancellation()
            if FileManager.default.fileExists(atPath: pending.path) { try FileManager.default.removeItem(at: pending) }
            do {
                try await writer(pending)
                try Task.checkCancellation()
                let attributes = try FileManager.default.attributesOfItem(atPath: pending.path)
                guard ((attributes[.size] as? NSNumber)?.int64Value ?? 0) > 0 else {
                    throw ClientError.message("Photos returned an empty original file.")
                }
                try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: pending.path)
                try FileManager.default.moveItem(at: pending, to: destination)
                return
            } catch {
                if error is CancellationError || Task.isCancelled { throw CancellationError() }
                if let transport = error as? URLError { throw transport }
                let native = error as NSError
                guard native.domain == PHPhotosErrorDomain, native.code == -1, attempt + 1 < attempts else {
                    throw PhotoLibraryOriginalReadError(error)
                }
                // PhotoKit can briefly fail while materializing an iCloud original.
                // Retry the same resource; never substitute a rendered or edited version.
                try await Task.sleep(for: .milliseconds(250))
            }
        }
    }
}
