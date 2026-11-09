import SwiftUI

@MainActor @Observable final class TimelineState {
    var groups: [TimelineGroup] = []
    var next: String?
    var previous: String?
    var hasOlder = false
    var hasNewer = false
    var loaded = false
    var loading = false
    var error: String?
    var search = ""
    var period = "day"
    var anchor: Date = .now
    var useAnchor = false
    var selection: Set<Int64> = []
    var selecting = false
    private var generation = 0
    var media: [Media] { groups.flatMap(\.media) }

    func load(api: APIClient, destination: Destination, reset: Bool, direction: String) async {
        guard reset || !loading else { return }
        generation += 1
        let requestGeneration = generation
        loading = true
        defer { if generation == requestGeneration { loading = false } }
        do {
            let type: String? = destination == .videos ? "video" : destination == .timeline ? nil : "image"
            let classification: String? = destination == .screenshots ? "screenshot" : destination == .documents ? "document" : nil
            let response: TimelineResponse = try await api.call("timeline/list", TimelineRequest(cursor: reset ? nil : direction == "older" ? next : previous, limit: 100,
                groupBy: period, search: search.trimmingCharacters(in: .whitespacesAndNewlines), mediaType: type,
                classification: classification, direction: direction, anchorDate: (useAnchor ? anchor : Date()).ISO8601Format()))
            guard requestGeneration == generation else { return }
            if reset { groups = response.groups; selection.removeAll() }
            else {
                let all = direction == "older" ? groups + response.groups : response.groups + groups
                var seen = Set<Int64>()
                var combined: [TimelineGroup] = []
                for group in all {
                    let items = group.media.filter { seen.insert($0.id).inserted }
                    if let i = combined.firstIndex(where: { $0.date == group.date }) { combined[i].media += items }
                    else if !items.isEmpty { combined.append(TimelineGroup(date: group.date, media: items)) }
                }
                groups = combined
            }
            if reset || direction == "older" { next = response.nextCursor; hasOlder = response.hasOlder }
            if reset || direction == "newer" { previous = response.previousCursor; hasNewer = response.hasNewer }
            loaded = true; error = nil
        } catch is CancellationError { }
        catch { if requestGeneration == generation { self.error = error.localizedDescription } }
    }

    func remove(_ ids: Set<Int64>) {
        for i in groups.indices { groups[i].media.removeAll { ids.contains($0.id) } }
        groups.removeAll { $0.media.isEmpty }
        selection.subtract(ids)
    }
}

struct TimelineView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openLibrary) private var openLibrary
    @Environment(\.pageActive) private var active
    let destination: Destination
    @State private var state = TimelineState()
    @State private var viewer: Media?
    @State private var filters = false
    @State private var albumPicker = false
    @State private var deleting = false
    @State private var visible: Set<Int64> = []
    @State private var searchDraft = ""
    @State private var editingSearch = false
    @FocusState private var searchFocused: Bool
    private var visibleDate: String {
        state.groups.first(where: { $0.media.contains(where: { visible.contains($0.id) }) })?.date ?? state.groups.first?.date ?? "Choose date"
    }
    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if state.hasNewer { Button("Load newer") { load(reset: false, direction: "newer") } }
                MediaGrid(media: state.media, selection: $state.selection, selecting: state.selecting, visibilityChanged: { id, shown in
                    if shown { visible.insert(id) } else { visible.remove(id) }
                }) { viewer = $0 }
                if state.hasOlder { Button("Load older") { load(reset: false, direction: "older") }.onAppear { load(reset: false, direction: "older") } }
                if state.loading { ProgressView().frame(maxWidth: .infinity).padding() }
                if let error = state.error { FailureView(message: error) { load(reset: !state.loaded, direction: "older") } }
                if state.loaded && state.media.isEmpty { ContentUnavailableView("No moments yet", systemImage: "photo.stack", description: Text("Back up your device or change the filters.")) }
            }
        }
        .scrollEdgeEffectHidden(true, for: .top)
        .toolbarBackground(.hidden, for: .navigationBar)
        .onChange(of: model.trashedMedia) { _, ids in state.remove(ids) }
        .onChange(of: state.search) { old, new in if !old.isEmpty && new.isEmpty { load(reset: true, direction: "older") } }
        .refreshable { await state.load(api: model.api, destination: destination, reset: true, direction: "older") }
        .task { if !state.loaded { load(reset: true, direction: "older") } }
        .toolbar {
            ToolbarItem(placement: .principal) {
                Button(visibleDate, systemImage: "calendar") { filters = true }
                    .labelStyle(.titleAndIcon).font(.caption).accessibilityLabel("Choose date: \(visibleDate)")
                    .buttonStyle(FloatingGlassButtonStyle()).accessibilityIdentifier("timelineDate")
            }.sharedBackgroundVisibility(.hidden)
            ToolbarItem(placement: .topBarTrailing) {
                Button(state.selecting ? "Done" : "Select") {
                    state.selecting.toggle()
                    if !state.selecting { state.selection.removeAll() }
                }
                    .buttonStyle(FloatingGlassButtonStyle()).accessibilityIdentifier("timelineSelection")
            }.sharedBackgroundVisibility(.hidden)
        }
        .safeAreaBar(edge: .bottom) {
            if active {
            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    Button("Library", systemImage: "line.3.horizontal", action: openLibrary)
                        .labelStyle(.iconOnly).buttonStyle(FloatingGlassButtonStyle())
                    if state.selecting {
                        TrailingSelectionActions {
                            HStack(spacing: 10) {
                                Button("All") { state.selection = Set(state.media.map(\.id)) }
                                Text("\(state.selection.count) selected")
                                Button("Add to album", systemImage: "rectangle.stack.badge.plus") { albumPicker = true }.labelStyle(.iconOnly).disabled(state.selection.isEmpty)
                                Button("Trash", systemImage: "trash", role: .destructive) { deleting = true }.labelStyle(.iconOnly).disabled(state.selection.isEmpty)
                            }.font(.caption).padding(.horizontal, 12).frame(height: 44).glassEffect(in: .capsule)
                        }
                    } else {
                    Spacer(minLength: 0)
                    if editingSearch || !state.search.isEmpty {
                        TextField("Search photos", text: $searchDraft).focused($searchFocused).submitLabel(.search)
                            .onSubmit(submitSearch).padding(.horizontal, 14).frame(height: FloatingGlassControl.height).glassEffect(in: .capsule)
                            .accessibilityIdentifier("timelineSearch")
                    } else {
                        HStack(spacing: 0) {
                            ForEach(["day", "week", "month", "year"], id: \.self) { period in
                                Button(period.capitalized) {
                                    if state.period != period { state.period = period; load(reset: true, direction: "older") }
                                }.font(.caption.weight(.semibold)).frame(minWidth: 40).frame(height: FloatingGlassControl.height)
                                    .background(state.period == period ? Color.primary.opacity(0.14) : .clear, in: .capsule)
                                    .accessibilityAddTraits(state.period == period ? .isSelected : [])
                            }
                        }.buttonStyle(.plain).glassEffect(in: .capsule)
                    }
                    Spacer(minLength: 0)
                    Button(state.search.isEmpty ? "Search" : "Clear search", systemImage: state.search.isEmpty ? "magnifyingglass" : "xmark") {
                        if !state.search.isEmpty { state.search = ""; searchDraft = ""; editingSearch = false; searchFocused = false }
                        else if editingSearch { submitSearch() }
                        else { editingSearch = true; searchFocused = true }
                    }.labelStyle(.iconOnly).buttonStyle(FloatingGlassButtonStyle())
                    }
                }.padding(12)
            }
            }
        }
        .sheet(isPresented: $filters) {
            TimelineFilterView(period: state.period, useAnchor: state.useAnchor, anchor: state.anchor) { period, useAnchor, anchor in
                state.period = period; state.useAnchor = useAnchor; state.anchor = anchor
                filters = false; load(reset: true, direction: "older")
            }.presentationDetents([.medium])
        }
        .sheet(isPresented: $albumPicker) { AlbumPicker(mediaIds: Array(state.selection)) }
        .fullScreenCover(item: $viewer) { item in MediaViewer(media: state.media, initial: item.id, removed: { state.remove($0) }) }
        .confirmationDialog("Move \(state.selection.count) items to Trash?", isPresented: $deleting, titleVisibility: .visible) {
            Button("Move to Trash", role: .destructive) {
                let ids = state.selection
                Task { await model.perform { try await model.trash(ids); state.remove(ids) } }
            }
        }
    }
    private func submitSearch() {
        state.search = searchDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        editingSearch = false; searchFocused = false
        load(reset: true, direction: "older")
    }
    private func load(reset: Bool, direction: String) { Task { await state.load(api: model.api, destination: destination, reset: reset, direction: direction) } }
}

private struct TimelineFilterView: View {
    @Environment(\.dismiss) private var dismiss
    @State var period: String
    @State var useAnchor: Bool
    @State var anchor: Date
    let apply: (String, Bool, Date) -> Void
    var body: some View {
        NavigationStack {
            Form {
                Picker("Group by", selection: $period) { ForEach(["day", "week", "month", "year"], id: \.self) { Text($0.capitalized).tag($0) } }
                Toggle("Jump to date", isOn: $useAnchor)
                if useAnchor { DatePicker("Date", selection: $anchor, displayedComponents: .date) }
            }.navigationTitle("Timeline filters").toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Apply") { apply(period, useAnchor, anchor) } }
            }
        }
    }
}
