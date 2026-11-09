import SwiftUI

struct CollectionCover: View {
    let paths: [String]
    let title: String?
    let subtitle: String?
    let badge: String
    let aspect: CGFloat
    var body: some View {
        Color.clear.aspectRatio(aspect, contentMode: .fit)
            .overlay {
                if paths.count > 1 {
                    VStack(spacing: 1) {
                        HStack(spacing: 1) { thumbnail(0); thumbnail(1) }
                        HStack(spacing: 1) { thumbnail(2); thumbnail(3) }
                    }
                } else { thumbnail(0) }
            }
            .overlay(alignment: .bottom) {
                HStack(alignment: .bottom) {
                    if let title {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(title).font(.headline).lineLimit(2)
                            if let subtitle { Text(subtitle).font(.caption).lineLimit(2) }
                        }
                    }
                    Spacer(minLength: 2)
                    Text(badge).font(.caption.weight(.semibold)).padding(6).background(.black.opacity(0.6), in: .capsule)
                }.foregroundStyle(.white).padding(10)
                    .background(LinearGradient(colors: [.clear, .black.opacity(0.75)], startPoint: .top, endPoint: .bottom))
            }
            .clipShape(.rect(cornerRadius: 12))
    }
    @ViewBuilder private func thumbnail(_ index: Int) -> some View {
        if paths.indices.contains(index) { RemoteImage(path: paths[index]) }
        else { Rectangle().fill(.quaternary).overlay { Image(systemName: "photo") } }
    }
}

struct DuplicateMediaCard: View {
    let item: Media
    @Binding var selected: Bool
    let open: () -> Void
    private var dimensions: String {
        guard let width = item.width, let height = item.height else { return "Unknown dimensions" }
        return "\(width) × \(height)"
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button(action: open) { CollectionCover(paths: ["media/\(item.id)/thumbnail"], title: nil, subtitle: nil, badge: item.mediaType, aspect: 1) }.buttonStyle(.plain)
                .overlay(alignment: .topLeading) {
                    Button { selected.toggle() } label: { SelectionMark(selected: selected).frame(width: 44, height: 44).contentShape(Rectangle()) }
                        .buttonStyle(.plain).accessibilityLabel("Select \(item.originalFilename)")
                }
            Text(item.originalFilename).font(.caption).lineLimit(2)
            Text(item.fileSize.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .binary) } ?? "Unknown size").font(.caption)
            Text(dimensions).font(.caption)
            Text(item.dateTaken ?? item.createdAt).font(.caption2).foregroundStyle(.secondary)
        }
    }
}

enum CollectionPolicy {
    static func placeColumns(width: CGFloat) -> Int {
        switch width { case ..<360: 1; case ..<600: 2; case ..<840: 3; case ..<1200: 4; default: 5 }
    }
    static func comparisonColumns(width: CGFloat) -> Int { width < 560 ? 2 : width < 840 ? 3 : 4 }
    static func keepsOriginalInEveryGroup(_ groups: [DeduplicateGroup], selected: Set<Int64>) -> Bool {
        !groups.contains { !$0.items.isEmpty && $0.items.allSatisfy { selected.contains($0.id) } }
    }
    static func trashDaysRemaining(deletedAt: String, now: Date) -> Int? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let deleted = formatter.date(from: deletedAt) ?? ISO8601DateFormatter().date(from: deletedAt)
        guard let deleted else { return nil }
        return max(0, Int(ceil(deleted.addingTimeInterval(30 * 86400).timeIntervalSince(now) / 86400)))
    }
}
