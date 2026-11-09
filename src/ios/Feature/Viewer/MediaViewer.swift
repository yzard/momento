import SwiftUI
import AVKit
import Photos

struct MediaPresentation: Identifiable {
    let media: [Media]
    let initial: Int64
    var id: Int64 { initial }
}

struct MediaViewer: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let media: [Media]
    let initial: Int64
    let removed: (Set<Int64>) -> Void
    @State private var selected: Int64 = 0
    @State private var details = false
    @State private var deleting = false
    @State private var album = false
    @State private var shareURL: URL?
    @State private var share = false
    @State private var playback = VideoPlayback()
    @Environment(\.scenePhase) private var scenePhase
    @State private var controls = true
    @State private var busy = false
    @State private var removedIds: Set<Int64> = []
    @State private var size: CGSize = .zero
    @State private var infoWidth: CGFloat = 320
    @GestureState private var infoDrag: CGFloat = 0
    @State private var interaction = 0
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOver
    private var remaining: [Media] { media.filter { !removedIds.contains($0.id) } }
    private var current: Media? { remaining.first { $0.id == selected } }
    private var wide: Bool { size.width > size.height && size.width >= 600 }
    private var maximumPanelWidth: CGFloat { min(size.width * 0.6, max(240, size.width - 326)) }
    private var panelWidth: CGFloat { min(max(240, infoWidth - infoDrag), maximumPanelWidth) }
    private var takenDate: Date? {
        let value = current?.dateTaken ?? current?.createdAt ?? ""
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: value) ?? ISO8601DateFormatter().date(from: value)
    }
    private var place: String? {
        [current?.locationCity, current?.locationState, current?.locationCountry]
            .compactMap { $0 }.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }
    init(media: [Media], initial: Int64, removed: @escaping (Set<Int64>) -> Void) {
        self.media = media
        self.initial = initial
        self.removed = removed
        _selected = State(initialValue: initial)
    }
    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
            HStack(spacing: 0) {
            ZStack {
                Color.black.ignoresSafeArea()
                TabView(selection: $selected) {
                    ForEach(remaining) { item in
                        MediaPage(item: item, active: item.id == selected, playback: playback, controls: controls, toggleControls: { controls.toggle(); interaction += 1 }).tag(item.id)
                    }
                }.tabViewStyle(.page(indexDisplayMode: .never))
                if busy { ProgressView().padding().glassEffect() }
            }
            .frame(minWidth: 0, maxWidth: .infinity)
            .safeAreaBar(edge: .top) { if controls { header } }
            .safeAreaBar(edge: .bottom) { if controls { footer } }
            if details && wide, let current {
                Rectangle().fill(.secondary).frame(width: 6).contentShape(Rectangle())
                    .gesture(DragGesture().updating($infoDrag) { value, state, _ in state = value.translation.width }
                        .onEnded { infoWidth = min(max(240, infoWidth - $0.translation.width), maximumPanelWidth) })
                    .accessibilityLabel("Resize information panel")
                MediaInformation(item: current, embedded: true, close: { details = false }).frame(width: panelWidth)
            }
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
            }
            .onGeometryChange(for: CGSize.self) { $0.size } action: { size = $0 }
            .toolbar(.hidden, for: .navigationBar)
            .onChange(of: selected) { _, _ in interaction += 1; controls = true }
            .task(id: "\(interaction):\(controls):\(details):\(album):\(share):\(busy):\(deleting):\(voiceOver)") {
                guard controls && current?.mediaType != "video" && !details && !album && !share && !busy && !deleting && !voiceOver else { return }
                do { try await Task.sleep(for: .seconds(4)); controls = false } catch { }
            }
            .sheet(isPresented: Binding(get: { details && !wide }, set: { if !wide { details = $0 } })) { if let current { MediaInformation(item: current, embedded: false, close: { details = false }) } }
            .sheet(isPresented: $album) { AlbumPicker(mediaIds: [selected]) }
            .sheet(isPresented: $share, onDismiss: cleanupShare) { if let shareURL { ShareSheet(url: shareURL) } }
            .confirmationDialog("Move this item to Trash?", isPresented: $deleting) {
                Button("Move to Trash", role: .destructive) { Task { await model.perform {
                    let id = selected
                    let index = remaining.firstIndex(where: { $0.id == id }) ?? 0
                    try await model.trash([id]); removedIds.insert(id); removed([id])
                    if remaining.isEmpty { dismiss() }
                    else { selected = remaining[min(index, remaining.count - 1)].id }
                } } }
            }
            .task(id: selected) {
                guard let current, current.mediaType == "video" else { playback.stop(); return }
                let id = current.id
                playback.load(media: current, api: model.api)
                if let cache = model.cache,
                   let data = try? await cache.data(path: "media/\(id)/thumbnail", api: model.api),
                   !Task.isCancelled, playback.mediaID == id { playback.poster = UIImage(data: data) }
                while !Task.isCancelled && playback.mediaID == id {
                    playback.synchronize()
                    do { try await Task.sleep(for: .milliseconds(200)) } catch { break }
                }
            }
            .onChange(of: scenePhase) { _, phase in if phase != .active { playback.pause() } }
            .onDisappear { playback.stop(); cleanupShare() }
        }.preferredColorScheme(.dark)
    }

    private var header: some View {
        HStack(spacing: 12) {
            Button("Close", systemImage: "chevron.left") { dismiss() }
                .labelStyle(.iconOnly).buttonStyle(FloatingGlassButtonStyle())
                .accessibilityIdentifier("viewerClose")
            Spacer(minLength: 0)
            Button { details.toggle() } label: {
                VStack(spacing: 1) {
                    Text(place ?? takenDate?.formatted(date: .abbreviated, time: .omitted) ?? "Unknown date")
                        .font(.subheadline.weight(.semibold))
                    Text(takenDate?.formatted(date: place == nil ? .omitted : .abbreviated, time: .shortened) ?? current?.dateTaken ?? current?.createdAt ?? "")
                        .font(.caption)
                }.lineLimit(1).minimumScaleFactor(0.8).padding(.horizontal, 8)
            }.buttonStyle(FloatingGlassButtonStyle()).layoutPriority(1)
                .accessibilityIdentifier("viewerDateLocation")
            Spacer(minLength: 0)
            Menu {
                Button("Information", systemImage: "info.circle") { details = true }
                Button("Add to album", systemImage: "rectangle.stack.badge.plus") { album = true }
                Button("Share original", systemImage: "square.and.arrow.up") { Task { await export(saveToPhotos: false) } }
                Button("Save to Photos", systemImage: "square.and.arrow.down") { Task { await export(saveToPhotos: true) } }
                Button("Move to Trash", systemImage: "trash", role: .destructive) { deleting = true }
            } label: { Label("More", systemImage: "ellipsis").labelStyle(.iconOnly) }
                .buttonStyle(FloatingGlassButtonStyle()).disabled(busy)
                .accessibilityIdentifier("viewerMore")
        }.font(.title3).foregroundStyle(.white).padding(.horizontal, 16).padding(.vertical, 8)
    }

    private var footer: some View {
        VStack(spacing: 8) {
            ScrollViewReader { proxy in
                ScrollView(.horizontal) {
                    HStack(spacing: 4) {
                        ForEach(remaining) { item in
                            Button { selected = item.id } label: {
                                RemoteImage(path: "media/\(item.id)/thumbnail/tiny")
                                    .frame(width: selected == item.id ? 40 : 28, height: 44)
                                    .clipShape(.rect(cornerRadius: 4))
                                    .overlay { RoundedRectangle(cornerRadius: 4).stroke(selected == item.id ? .white : .clear, lineWidth: 1.5) }
                                    .opacity(selected == item.id ? 1 : 0.7)
                            }.buttonStyle(.plain).id(item.id)
                                .accessibilityIdentifier("viewerThumbnail.\(item.id)")
                                .accessibilityLabel("View \(item.originalFilename)")
                                .accessibilityAddTraits(selected == item.id ? .isSelected : [])
                        }
                    }.padding(.horizontal, 16).padding(.vertical, 4)
                }.scrollIndicators(.hidden).accessibilityIdentifier("viewerFilmstrip")
                    .onAppear { proxy.scrollTo(selected, anchor: .center) }
                    .onChange(of: selected) { _, id in withAnimation { proxy.scrollTo(id, anchor: .center) } }
            }
            HStack(spacing: 8) {
                Button("Share", systemImage: "square.and.arrow.up") { Task { await export(saveToPhotos: false) } }
                    .buttonStyle(FloatingGlassButtonStyle()).accessibilityIdentifier("viewerShare")
                Spacer(minLength: 0)
                HStack(spacing: 4) {
                    Button { album = true } label: { Label("Albums", systemImage: "rectangle.stack").frame(width: 44, height: 44) }
                    Button { details.toggle() } label: { Label("Information", systemImage: "info.circle").frame(width: 44, height: 44) }
                    Button { Task { await export(saveToPhotos: true) } } label: { Label("Save to Photos", systemImage: "square.and.arrow.down").frame(width: 44, height: 44) }
                }.buttonStyle(.plain).padding(.horizontal, 6)
                    .glassEffect(.regular.interactive(), in: .capsule)
                    .accessibilityIdentifier("viewerActions")
                Spacer(minLength: 0)
                Button("Trash", systemImage: "trash") { deleting = true }
                    .buttonStyle(FloatingGlassButtonStyle()).accessibilityIdentifier("viewerTrash")
            }.labelStyle(.iconOnly).font(.title2).foregroundStyle(.white)
                .buttonBorderShape(.circle).disabled(busy).padding(.horizontal, 16).padding(.bottom, 8)
        }
    }

    private func export(saveToPhotos: Bool) async {
        guard let current else { return }; playback.pause(); busy = true; defer { busy = false }
        await model.perform {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let file = directory.appending(path: (current.originalFilename as NSString).lastPathComponent)
            shareURL = file
            try await model.api.download(path: "media/\(current.id)/original", destination: file)
            if saveToPhotos {
                let authorization = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
                guard authorization == .authorized || authorization == .limited else { throw ClientError.message("Allow Photos access to save this original.") }
                try await PhotoLibrary.saveOriginal(file, isVideo: current.mediaType == "video")
                cleanupShare()
            } else { share = true }
        }
    }
    private func cleanupShare() {
        if let shareURL { try? FileManager.default.removeItem(at: shareURL.deletingLastPathComponent()) }
        shareURL = nil
    }
}

private struct MediaPage: View {
    @Environment(AppModel.self) private var model
    let item: Media
    let active: Bool
    let playback: VideoPlayback
    let controls: Bool
    let toggleControls: () -> Void
    @State private var image: UIImage?
    @State private var error: String?
    @State private var zoom: CGFloat = 1
    @GestureState private var magnification: CGFloat = 1
    var body: some View {
        GeometryReader { viewport in
            ZStack {
                if item.mediaType == "video" {
                    if active && playback.mediaID == item.id {
                        VideoPlaybackView(playback: playback, controls: controls, toggleControls: toggleControls)
                    } else { RemoteImage(path: "media/\(item.id)/thumbnail") }
                } else if let image {
                    ScrollView([.horizontal, .vertical]) {
                        Image(uiImage: image).resizable().scaledToFit()
                            .frame(width: viewport.size.width, height: viewport.size.height)
                            .scaleEffect(zoom * magnification)
                            .accessibilityLabel("Media preview").accessibilityIdentifier("viewerImage")
                            .gesture(MagnifyGesture().updating($magnification) { value, state, _ in state = value.magnification }.onEnded { zoom = min(5, max(1, zoom * $0.magnification)) })
                            .onTapGesture(count: 2) { withAnimation { zoom = zoom > 1 ? 1 : 2.5 } }
                    }.simultaneousGesture(TapGesture().onEnded(toggleControls))
                } else if let error { Text(error).foregroundStyle(.white).padding() }
                else { ProgressView().tint(.white) }
            }.frame(width: viewport.size.width, height: viewport.size.height)
        }
        .task(id: active) {
            guard active, item.mediaType != "video", image == nil else { return }
            do {
                if let cache = model.cache {
                    let data = try await cache.data(path: "media/\(item.id)/preview", api: model.api)
                    guard let decoded = UIImage(data: data) else { throw ClientError.message("This image could not be decoded.") }
                    image = decoded
                }
            } catch is CancellationError { }
            catch { self.error = error.localizedDescription }
        }
    }
}

private struct MediaInformation: View {
    let item: Media
    let embedded: Bool
    let close: () -> Void
    var body: some View {
        if embedded {
            VStack(spacing: 0) {
                HStack {
                    Text("Information").font(.headline)
                    Spacer()
                    Button("Done", action: close).buttonStyle(FloatingGlassButtonStyle())
                }.padding(16)
                information
            }.background(Color(uiColor: .systemGroupedBackground))
        } else {
            NavigationStack {
                information.navigationTitle("Information").navigationBarTitleDisplayMode(.inline)
                    .toolbar { Button("Done", action: close) }
            }
        }
    }
    private var information: some View {
            List {
                LabeledContent("Media ID", value: String(item.id))
                LabeledContent("Filename", value: item.originalFilename)
                LabeledContent("Taken", value: item.dateTaken ?? "Unknown")
                LabeledContent("Added", value: item.createdAt)
                LabeledContent("Type", value: item.mimeType ?? item.mediaType)
                if let size = item.fileSize { LabeledContent("Size", value: ByteCountFormatter.string(fromByteCount: size, countStyle: .file)) }
                if let width = item.width, let height = item.height { LabeledContent("Dimensions", value: "\(width) × \(height)") }
                if let duration = item.durationSeconds { LabeledContent("Duration", value: Duration.seconds(duration).formatted(.time(pattern: .hourMinuteSecond))) }
                if let make = item.cameraMake { LabeledContent("Camera", value: "\(make) \(item.cameraModel ?? "")") }
                if let lens = item.lensModel { LabeledContent("Lens", value: lens) }
                if let make = item.lensMake { LabeledContent("Lens make", value: make) }
                if let iso = item.iso { LabeledContent("ISO", value: String(iso)) }
                if let exposure = item.exposureTime { LabeledContent("Exposure", value: exposure) }
                if let f = item.fNumber { LabeledContent("Aperture", value: "f/\(f)") }
                if let length = item.focalLength { LabeledContent("Focal length", value: "\(length) mm") }
                if let length = item.focalLength35mm { LabeledContent("35 mm equivalent", value: "\(length) mm") }
                if item.locationCity != nil || item.locationState != nil || item.locationCountry != nil { LabeledContent("Place", value: [item.locationCity, item.locationState, item.locationCountry].compactMap { $0 }.joined(separator: ", ")) }
                if let lat = item.gpsLatitude, let lon = item.gpsLongitude { LabeledContent("GPS", value: "\(lat), \(lon)") }
                if let altitude = item.gpsAltitude { LabeledContent("Altitude", value: "\(altitude) m") }
                if let codec = item.videoCodec { LabeledContent("Video codec", value: codec) }
                if let keywords = item.keywords { LabeledContent("Keywords", value: keywords) }
                if let hash = item.contentHash { LabeledContent("SHA-256", value: hash).font(.caption).textSelection(.enabled) }
            }
    }
}

private struct ShareSheet: UIViewControllerRepresentable {
    let url: URL
    func makeUIViewController(context: Context) -> UIActivityViewController { UIActivityViewController(activityItems: [url], applicationActivities: nil) }
    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
