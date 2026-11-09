import Foundation
import SwiftUI

func backupByteCount(_ bytes: Int64) -> String {
    let value = max(0, bytes)
    let gigabytes = value >= 1_000_000_000
    let unit: Int64 = gigabytes ? 1_000_000_000 : 1_000_000
    let hundredths = value / unit * 100 + (value % unit * 100 + unit / 2) / unit
    let whole = hundredths / 100
    let fraction = hundredths % 100
    let number = fraction == 0 ? "\(whole)" : fraction % 10 == 0 ? "\(whole).\(fraction / 10)" : "\(whole)." + String(format: "%02d", Int(fraction))

    return "\(number) \(gigabytes ? "GB" : "MB")"
}

func backupFileStatus(filename: String, uploaded: Int64, total: Int64) -> String {
    "Backing up \(filename) \(backupByteCount(uploaded))/\(backupByteCount(total))"
}

func backupUploadFailure(filename: String, reason: String) -> String {
    "\(filename): Upload failed — \(reason)"
}

struct BackupProgressContent: View {
    let status: String
    let running: Bool
    let completed: Int
    let total: Int
    let errors: [String]
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(status).frame(maxWidth: .infinity, alignment: .leading).accessibilityIdentifier("backupStatus")
            if running {
                HStack(spacing: 8) {
                    ProgressView(value: Double(completed), total: Double(max(1, total)))
                        .accessibilityLabel("Backup media progress")
                    Text("\(completed)/\(total)").font(.caption.monospacedDigit())
                        .accessibilityIdentifier("backupMediaCount")
                }
            }
            ForEach(Array(errors.enumerated()), id: \.offset) { _, message in
                Text(message).foregroundStyle(.red).frame(maxWidth: .infinity, alignment: .leading).accessibilityIdentifier("backupUploadError")
            }
        }.font(.subheadline).foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .accessibilityElement(children: .contain).accessibilityIdentifier("backupProgressContent")
    }
}
