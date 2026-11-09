import SwiftUI

struct RemoteImage: View {
    @Environment(AppModel.self) private var model
    let path: String
    @State private var image: UIImage?
    @State private var failed = false
    var body: some View {
        ZStack {
            Rectangle().fill(.quaternary)
            if let image { Image(uiImage: image).resizable().scaledToFill() }
            else { Image(systemName: failed ? "photo.badge.exclamationmark" : "photo").foregroundStyle(.secondary) }
        }.clipped().task(id: "\(path):\(model.cache != nil)") {
            do {
                if let cache = model.cache { image = UIImage(data: try await cache.data(path: path, api: model.api)) }
            } catch is CancellationError { }
            catch { failed = true }
        }
    }
}

struct MediaGrid: View {
    @State private var width: CGFloat = 390
    let media: [Media]
    @Binding var selection: Set<Int64>
    let selecting: Bool
    let visibilityChanged: ((Int64, Bool) -> Void)?
    let open: (Media) -> Void
    static func columns(width: CGFloat) -> Int {
        switch width {
        case ..<360: 2
        case ..<480: 3
        case ..<600: 4
        case ..<840: 5
        case ..<1200: 6
        default: 8
        }
    }
    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 1), count: Self.columns(width: width)), spacing: 1) {
            ForEach(media) { item in
                Button {
                    if selecting { if !selection.insert(item.id).inserted { selection.remove(item.id) } }
                    else { open(item) }
                } label: {
                    Color.clear.aspectRatio(1, contentMode: .fit)
                        .overlay { RemoteImage(path: "media/\(item.id)/thumbnail").allowsHitTesting(false) }
                        .overlay(alignment: .topLeading) {
                            if selecting {
                                SelectionMark(selected: selection.contains(item.id)).padding(6)
                            } else if item.mediaType == "video" {
                                Label(item.durationSeconds.map { Duration.seconds($0).formatted(.time(pattern: .minuteSecond)) } ?? "Video", systemImage: "play.fill")
                                    .font(.caption).padding(5).background(.ultraThinMaterial, in: Capsule()).padding(5)
                            }
                        }.clipped().contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel(item.originalFilename)
                    .onScrollVisibilityChange(threshold: 0.1) { visibilityChanged?(item.id, $0) }
            }
        }.onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }
}

struct FailureView: View {
    let message: String
    let retry: () -> Void
    var body: some View {
        ContentUnavailableView {
            Label("Could not load", systemImage: "wifi.exclamationmark")
        } description: { Text(message) } actions: { Button("Retry", action: retry).buttonStyle(.glass) }
    }
}
