import Foundation
import SwiftUI
import UIKit
import Testing
@testable import Momento

@Suite(.serialized) @MainActor struct BackupPresentationTests {
    @Test func byteCountsUseAtLeastMBAndAtMostTwoDecimalPlaces() {
        let cases: [(Int64, String)] = [(-1, "0 MB"), (0, "0 MB"), (999, "0 MB"),
            (12_340, "0.01 MB"), (1_000_000, "1 MB"), (2_675_000, "2.68 MB"),
            (12_345_678, "12.35 MB"), (1_000_000_000, "1 GB"), (2_345_678_901, "2.35 GB")]
        for (bytes, expected) in cases { #expect(backupByteCount(bytes) == expected) }
        #expect(backupFileStatus(filename: "Original MOV.mov", uploaded: 12_340_000, total: 2_000_000_000) ==
            "Backing up Original MOV.mov 12.34 MB/2 GB")
        #expect(backupByteCount(Int64.max) == "9223372036.85 GB")
    }

    @Test(arguments: [390.0, 900.0]) func progressAndLogsFillTheCompactAndExpandedSettingsWidth(_ width: Double) async throws {
        var measured = CGSize.zero
        let root = BackupProgressContent(status: "Backing up Original.MOV 12.34 MB/2 GB", running: true,
            completed: 3, total: 10, errors: ["Original.MOV: Upload failed — Cannot Open"])
            .onGeometryChange(for: CGSize.self) { $0.size } action: { measured = $0 }
        let controller = UIHostingController(rootView: root)
        let previousKeyWindow = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).first { $0.isKeyWindow }
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: width, height: 700))
        window.rootViewController = controller
        window.makeKeyAndVisible()
        defer { window.isHidden = true; window.rootViewController = nil; previousKeyWindow?.makeKey() }
        controller.view.frame = window.bounds
        controller.view.layoutIfNeeded()
        let deadline = Date().addingTimeInterval(2)
        while measured.width == 0 && Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(abs(measured.width - width) < 1)
    }

    @Test func failuresKeepOriginalNamesAndExcludeFilteredMediaFromTheTotal() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-presentation-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let backup = try BackupController(api: APIClient(transport: URLSession.shared, session: APIClientTests.session()), root: root)
        var failed = BackupTests().record()
        failed.filename = "Original.MOV"; failed.state = .failed; failed.error = "Cannot Open"
        var excluded = failed; excluded.id = "excluded"; excluded.state = .excluded
        backup.records = [failed, excluded]
        backup.latestError = backupUploadFailure(filename: failed.filename, reason: "Cannot Open")
        #expect(backup.total == 1)
        #expect(backup.uploadErrors == ["Original.MOV: Upload failed — Cannot Open"])
        excluded.filename = "excluded.mov"; excluded.error = "Previous camera detection failure"
        backup.records = [failed, excluded]
        #expect(!backup.diagnostics.contains("Previous camera detection failure"))
        #expect(backup.diagnostics.contains("Original.MOV: failed"))
        #expect(backup.diagnostics.contains("Cannot Open"))
        await backup.stop()
    }
}
