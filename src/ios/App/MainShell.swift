import SwiftUI

private struct PageActiveKey: EnvironmentKey { static let defaultValue = true }
private struct OpenLibraryKey: EnvironmentKey { static let defaultValue: @MainActor () -> Void = {} }
extension EnvironmentValues {
    var openLibrary: @MainActor () -> Void {
        get { self[OpenLibraryKey.self] }
        set { self[OpenLibraryKey.self] = newValue }
    }
    var pageActive: Bool {
        get { self[PageActiveKey.self] }
        set { self[PageActiveKey.self] = newValue }
    }
}

enum Destination: String, CaseIterable, Identifiable {
    case timeline = "Timeline", photos = "Photos", videos = "Videos", screenshots = "Screenshots", documents = "Documents"
    case albums = "Albums", map = "Map", places = "Places", faces = "Faces", duplicates = "Deduplicate", trash = "Trash"
    case settings = "Settings", imports = "Import", metadata = "Metadata", ai = "AI", users = "User Management"
    var id: String { rawValue }
    var icon: String {
        switch self {
        case .timeline: "photo.on.rectangle.angled"
        case .photos: "photo"
        case .videos: "video"
        case .screenshots: "viewfinder"
        case .documents: "doc.text"
        case .albums: "rectangle.stack"
        case .map: "map"
        case .places: "mappin.and.ellipse"
        case .faces: "person.crop.square"
        case .duplicates: "square.on.square"
        case .trash: "trash"
        case .settings: "gearshape"
        case .imports: "square.and.arrow.down"
        case .metadata: "info.circle"
        case .ai: "sparkles"
        case .users: "person.2"
        }
    }
    var timeline: Bool { [.timeline, .photos, .videos, .screenshots, .documents].contains(self) }
    var admin: Bool { [.imports, .metadata, .ai, .users].contains(self) }
    @MainActor func available(_ model: AppModel) -> Bool {
        if admin { return model.session?.user.role == "admin" }
        switch self {
        case .screenshots: return model.capabilities?.features.screenshotDetection == true
        case .documents: return model.capabilities?.features.documentDetection == true
        case .faces: return model.capabilities?.features.faceDetection == true
        case .duplicates: return model.capabilities?.features.deduplicate == true
        default: return true
        }
    }
}

struct MainShell: View {
    @Environment(AppModel.self) private var model
    @SceneStorage private var selectedName: String
    private var selected: Destination {
        get { Destination(rawValue: selectedName) ?? .timeline }
        nonmutating set { selectedName = newValue.rawValue }
    }
    init(namespace: String) {
        _selectedName = SceneStorage(wrappedValue: Destination.timeline.rawValue, "navigation.destination.\(namespace)")
    }
    @State private var visited: Set<Destination> = [.timeline]
    @State private var navigationOpen = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                ZStack {
                    ForEach(Destination.allCases.filter { visited.contains($0) || $0 == selected }) { destination in
                        NavigationStack {
                            content(destination)
                                .accessibilityHidden(selected != destination || navigationOpen)
                                .environment(\.pageActive, selected == destination && !navigationOpen)
                                .environment(\.openLibrary, { navigationOpen = true })
                                .navigationTitle(destination.rawValue)
                                .toolbar(selected == destination && !navigationOpen ? .visible : .hidden, for: .navigationBar)
                                .toolbarBackground(.hidden, for: .navigationBar)
                                .scrollEdgeEffectHidden(true, for: .top)
                                .safeAreaInset(edge: .bottom, spacing: 0) {
                                    if !destination.timeline { Color.clear.frame(height: 68) }
                                }
                                .overlayPreferenceValue(SelectionActionsKey.self) { actions in
                                    if selected == destination && !navigationOpen && !destination.timeline {
                                        HStack(spacing: 8) {
                                            Button("Library", systemImage: "line.3.horizontal") { navigationOpen = true }
                                                .labelStyle(.iconOnly).buttonStyle(FloatingGlassButtonStyle())
                                            if let content = actions[destination] {
                                                TrailingSelectionActions { content }
                                            } else { Spacer() }
                                        }.padding(12).frame(maxHeight: .infinity, alignment: .bottom)
                                    }
                                }
                        }
                        .opacity(selected == destination ? 1 : 0)
                        .allowsHitTesting(selected == destination && !navigationOpen)
                        .accessibilityHidden(selected != destination || navigationOpen)
                    }
                }
                if navigationOpen {
                    Color.black.opacity(0.3).ignoresSafeArea()
                        .onTapGesture { navigationOpen = false }
                        .accessibilityLabel("Close navigation menu")
                        .accessibilityAddTraits(.isButton)
                    NavigationDrawer(selected: selected, select: { selected = $0; navigationOpen = false }, logout: {
                        navigationOpen = false
                        Task { await model.logout(localOnly: false) }
                    })
                        .frame(width: min(280, max(0, geometry.size.width - 48)))
                        .frame(maxHeight: .infinity)
                        .glassEffect(.regular, in: .rect(cornerRadius: 16))
                        .accessibilityAddTraits(.isModal)
                        .accessibilityAction(.escape) { navigationOpen = false }
                        .transition(.move(edge: .leading).combined(with: .opacity))
                        .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in
                            if value.translation.width < -60 && abs(value.translation.height) < 80 { navigationOpen = false }
                        })
                }
            }
            .simultaneousGesture(DragGesture(minimumDistance: 20).onEnded { value in
                if !navigationOpen && selected != .map && value.startLocation.x < 20 && value.translation.width > 60 && abs(value.translation.height) < 80 { navigationOpen = true }
            })
            .animation(reduceMotion ? nil : .snappy(duration: 0.25), value: navigationOpen)
        }
        .onChange(of: selected) { previous, destination in visited.formUnion([previous, destination]); navigationOpen = false }
    }
    @ViewBuilder private func content(_ destination: Destination) -> some View {
        if destination.timeline { TimelineView(destination: destination) }
        else {
            switch destination {
            case .settings: SettingsView()
            case .map: LibraryMapView()
            case .imports, .metadata, .ai, .users: AdminView(destination: destination)
            default: CollectionsView(destination: destination)
            }
        }
    }
}
