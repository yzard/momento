import SwiftUI

struct CollectionTarget: Identifiable {
    let destination: Destination
    let numericId: Int64
    let stringId: String
    let title: String
    var id: String { "\(destination):\(numericId):\(stringId)" }
}

struct CollectionsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.pageActive) private var active
    let destination: Destination
    @State private var albums: [Album] = []
    @State private var places: [Place] = []
    @State private var faces: [FaceGroup] = []
    @State private var duplicates: [DeduplicateGroup] = []
    @State private var trash: [TrashMedia] = []
    @State private var selection: Set<Int64> = []
    @State private var next: String?
    @State private var hasMore = false
    @State private var loaded = false
    @State private var loading = false
    @State private var error: String?
    @State private var target: CollectionTarget?
    @State private var creating = false
    @State private var action: String?
    @State private var viewer: MediaPresentation?
    @State private var width: CGFloat = 390
    @State private var trashSelecting = false
    @State private var duplicateStatus = "unknown"
    @State private var totalGroups: Int64 = 0
    @State private var totalMedia: Int64 = 0
    @State private var deletingAlbum: Album?
    private var duplicateRunning: Bool { ["queued", "running", "cancelling"].contains(duplicateStatus) }
    var body: some View {
        libraryContent
        .sheet(item: $target) { target in
            CollectionDetail(target: target, updatedAlbum: { album in
                if let i = albums.firstIndex(where: { $0.id == album.id }) { albums[i] = album }
            }, deletedAlbum: { id in albums.removeAll { $0.id == id } })
        }
        .sheet(isPresented: $creating) {
            AlbumEditor(album: nil, mediaIds: [], saved: createdAlbum)
        }
        .fullScreenCover(item: $viewer) { item in MediaViewer(media: item.media, initial: item.initial, removed: { ids in
            for i in duplicates.indices { duplicates[i].items.removeAll { ids.contains($0.id) } }
            duplicates.removeAll { $0.items.count < 2 }
        }) }
        .confirmationDialog("Confirm \(action ?? "action")?", isPresented: Binding(get: { action != nil }, set: { if !$0 { action = nil } }), titleVisibility: .visible) {
            if let action { Button(action.capitalized, role: action == "merge" ? nil : .destructive) { Task { await mutate(action) } } }
        } message: { Text(action == "delete" || action == "empty" ? "This permanently deletes server media and cannot be undone." : "Apply this action to the selected items.") }
        .confirmationDialog("Delete album?", isPresented: Binding(get: { deletingAlbum != nil }, set: { if !$0 { deletingAlbum = nil } }), titleVisibility: .visible) {
            if let album = deletingAlbum {
                Button("Delete \(album.name)", role: .destructive) { Task { await model.perform {
                    let _: MessageResponse = try await model.api.call("album/delete", AlbumIdRequest(albumId: album.id))
                    albums.removeAll { $0.id == album.id }; model.deletedAlbums.insert(album.id); deletingAlbum = nil
                } } }
            }
        } message: { Text("The album is removed. Its original media remain in your library.") }
    }

    private var libraryContent: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 16) {
                collectionContent
                if loading { ProgressView().frame(maxWidth: .infinity) }
                if hasMore { Button("Load more") { Task { await load(reset: false) } }.onAppear { Task { await load(reset: false) } } }
                if let error { FailureView(message: error) { Task { await load(reset: !loaded) } } }
                if loaded && albums.isEmpty && places.isEmpty && faces.isEmpty && duplicates.isEmpty && trash.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: destination.icon)
                }
            }.padding()
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .task { if !loaded { await load(reset: true) } }
        .task(id: active) {
            guard active && destination == .duplicates && model.session?.user.role == "admin" else { return }
            do {
                while !Task.isCancelled {
                    if !model.busy && action == nil { await loadDuplicateStatus() }
                    try await Task.sleep(for: .seconds(2))
                }
            } catch is CancellationError { }
            catch { self.error = error.localizedDescription }
        }
        .onChange(of: model.albumRevision) { _, _ in
            guard destination == .albums else { return }
            for album in model.albumUpdates.values {
                if let i = albums.firstIndex(where: { $0.id == album.id }) { albums[i] = album }
                else { albums.append(album) }
            }
        }
        .onChange(of: model.deletedAlbums) { _, ids in albums.removeAll { ids.contains($0.id) } }
        .onChange(of: model.trashedMedia) { _, ids in
            for i in duplicates.indices { duplicates[i].items.removeAll { ids.contains($0.id) } }
            duplicates.removeAll { $0.items.count < 2 }
            selection.subtract(ids)
            if loaded && [.albums, .trash, .places, .faces].contains(destination) { Task { await load(reset: true) } }
        }
        .refreshable { await load(reset: true) }
        .toolbar { toolbarContent }
        .preference(key: SelectionActionsKey.self, value: selection.isEmpty ? [:] : [destination: AnyView(selectionBar)])
    }

    @ToolbarContentBuilder private var toolbarContent: some ToolbarContent {
            ToolbarItemGroup(placement: .topBarTrailing) {
                if destination == .albums { Button("New album", systemImage: "plus") { creating = true } }
                if destination == .trash { Button("Empty Trash", role: .destructive) { action = "empty" }.disabled(trash.isEmpty) }
                if destination == .trash {
                    Button(trashSelecting ? "Done" : "Select") { trashSelecting.toggle(); if !trashSelecting { selection.removeAll() } }
                    if trashSelecting { Button(selection.count == trash.count ? "Clear all" : "Select all") { selection = selection.count == trash.count ? [] : Set(trash.map(\.id)) } }
                }
            }
            }

    @ViewBuilder private var selectionBar: some View {
            if !selection.isEmpty {
                HStack {
                    Text("\(selection.count) selected"); Spacer()
                    if destination == .faces {
                        Button("Merge") { action = "merge" }.disabled(selection.count < 2)
                        Button("Reject", role: .destructive) { action = "reject" }
                    } else if destination == .trash {
                        Button("Restore") { Task { await mutate("restore") } }
                        Button("Delete", role: .destructive) { action = "delete" }
                    } else { Button("Trash", role: .destructive) { action = "trash" } }
                    Button("Done") { selection.removeAll() }
                }.font(.caption).padding(.horizontal, 12).frame(height: 44).glassEffect(in: .capsule)
            }
            }

    @ViewBuilder private var collectionContent: some View {
        switch destination {
        case .albums: albumsContent
        case .places: placesContent
        case .faces: facesContent
        case .duplicates: duplicatesContent
        case .trash: trashContent
        default: EmptyView()
        }
    }
    private var albumsContent: some View {
        Group {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 156), spacing: 12)], spacing: 12) {
            ForEach(albums) { album in
                Button {
                    target = CollectionTarget(destination: .albums, numericId: album.id, stringId: "", title: album.name)
                } label: { CollectionCover(paths: album.thumbnailMediaIds.prefix(4).map { "media/\($0)/thumbnail" }, title: album.name, subtitle: nil, badge: "\(album.mediaCount) items", aspect: 1) }
                    .buttonStyle(.plain)
                    .overlay(alignment: .topTrailing) {
                        Button("Delete \(album.name)", systemImage: "trash") { deletingAlbum = album }
                            .labelStyle(.iconOnly).padding(8).buttonStyle(.glass)
                    }
            }
            }
        }
    }

    private var placesContent: some View {
        Group {
            LazyVGrid(columns: columns(CollectionPolicy.placeColumns(width: width)), spacing: 12) {
            ForEach(places) { place in
                Button {
                    target = CollectionTarget(destination: .places, numericId: 0, stringId: place.placeId, title: place.city)
                } label: { CollectionCover(paths: ["places/\(place.placeId)/thumbnail"], title: place.city,
                    subtitle: "\(place.state.map { $0 + ", " } ?? "")\(place.country)", badge: "\(place.mediaCount)", aspect: 1.5) }.buttonStyle(.plain)
            }
            }
        }
    }

    private var facesContent: some View {
        Group {
            LazyVGrid(columns: columns(MediaGrid.columns(width: width)), spacing: 8) {
            ForEach(faces) { face in
                Button {
                    target = CollectionTarget(destination: .faces, numericId: face.id, stringId: "", title: "Person \(face.id)")
                } label: { CollectionCover(paths: ["faces/groups/\(face.id)/thumbnail"], title: nil, subtitle: nil, badge: "\(face.mediaCount)", aspect: 1) }
                .buttonStyle(.plain).accessibilityLabel("Person \(face.id), \(face.mediaCount) photos")
                .overlay(alignment: .topLeading) {
                    Button { if !selection.insert(face.id).inserted { selection.remove(face.id) } } label: {
                        SelectionMark(selected: selection.contains(face.id)).frame(width: 44, height: 44).contentShape(Rectangle())
                    }.padding(2).buttonStyle(.plain).accessibilityLabel("Select person \(face.id)")
                }
            }
            }
        }
    }

    private var duplicatesContent: some View {
        Group {
            Text("\(totalGroups) similar groups · \(totalMedia) media").font(.headline)
            if model.session?.user.role == "admin" {
                Text("Processing: \(duplicateStatus)").foregroundStyle(.secondary)
                HStack {
                    Button(duplicateRunning ? "Cancel" : "Start") {
                        if duplicateRunning { action = "cancel" } else { Task { await runDuplicateAction("start") } }
                    }
                    Button("Clean", role: .destructive) { action = "clean" }.disabled(duplicateRunning)
                }.buttonStyle(.glass).disabled(model.busy)
            }
            ForEach(duplicates) { group in
                Text("Group \(group.clusterId) · \(group.items.count) items").font(.headline)
                LazyVGrid(columns: columns(CollectionPolicy.comparisonColumns(width: width)), alignment: .leading, spacing: 12) {
                    ForEach(group.items) { item in
                                DuplicateMediaCard(item: item, selected: Binding(get: { selection.contains(item.id) }, set: { if $0 { selection.insert(item.id) } else { selection.remove(item.id) } }), open: { viewer = MediaPresentation(media: group.items, initial: item.id) })
                    }
                }
                Button("Keep first, select the rest") { selection.formUnion(group.items.dropFirst().map(\.id)) }
                Button("View comparison") { if let first = group.items.first { viewer = MediaPresentation(media: group.items, initial: first.id) } }
            }
        }
    }

    private var trashContent: some View {
        Group {
            Text("Deleted memories remain here for 30 days before permanent removal.").font(.subheadline).foregroundStyle(.secondary)
            LazyVGrid(columns: columns(MediaGrid.columns(width: width)), spacing: 1) {
            ForEach(trash) { item in
                Button {
                    trashSelecting = true
                    if !selection.insert(item.id).inserted { selection.remove(item.id) }
                } label: {
                    CollectionCover(paths: ["trash/\(item.id)/thumbnail"], title: nil, subtitle: nil, badge: retention(item), aspect: 1)
                        .overlay(alignment: .topLeading) {
                            if trashSelecting { SelectionMark(selected: selection.contains(item.id)).padding(6) }
                        }
                }.buttonStyle(.plain).accessibilityLabel(item.originalFilename)
            }
            }
        }
    }

    private func createdAlbum(_ detail: AlbumDetail) {
        if let album = model.albumUpdates[detail.id], !albums.contains(where: { $0.id == detail.id }) { albums.append(album) }
    }
    private func columns(_ count: Int) -> [GridItem] { Array(repeating: GridItem(.flexible(), spacing: 8), count: count) }
    private func retention(_ item: TrashMedia) -> String {
        guard let days = CollectionPolicy.trashDaysRemaining(deletedAt: item.deletedAt, now: .now) else { return "30-day retention" }
        return days == 0 ? "Deleting soon" : "\(days) d left"
    }
    private func loadDuplicateStatus() async {
        do {
            let response: AiStatusResponse = try await model.api.call("ai/status", EmptyRequest())
            duplicateStatus = response.deduplicate.status
        } catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }
    private func runDuplicateAction(_ operation: String) async {
        guard model.session?.user.role == "admin", !model.busy else { return }
        await model.perform {
            let response: AiActionResponse = try await model.api.call("ai/deduplicate/\(operation)", EmptyRequest())
            if let failure = response.results.compactMap(\.error).first { throw ClientError.message(failure) }
            selection.removeAll(); action = nil
            await loadDuplicateStatus()
            await load(reset: true)
        }
    }

    private func load(reset: Bool) async {
        guard !loading else { return }; loading = true; defer { loading = false }
        do {
            let page = PageRequest(cursor: reset ? nil : next, limit: 60)
            switch destination {
            case .albums:
                let response: AlbumsResponse = try await model.api.call("album/list", EmptyRequest()); albums = response.albums
            case .places:
                let response: PlacesResponse = try await model.api.call("places/list", page)
                places = reset ? response.places : places + response.places; next = response.nextCursor; hasMore = response.hasMore
            case .faces:
                let response: FacesResponse = try await model.api.call("faces/groups/list", page)
                faces = reset ? response.groups : faces + response.groups; next = response.nextCursor; hasMore = response.hasMore
            case .duplicates:
                let response: DeduplicateGroupsResponse = try await model.api.call("duplicates/list", page)
                duplicates = reset ? response.groups : duplicates + response.groups; next = response.nextCursor; hasMore = response.hasMore
                totalGroups = response.totalGroups; totalMedia = response.totalMedia
            case .trash:
                let response: TrashResponse = try await model.api.call("trash/list", EmptyRequest()); trash = response.items
            default: break
            }
            loaded = true; error = nil
        } catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }

    private func mutate(_ operation: String) async {
        if destination == .duplicates && ["cancel", "clean"].contains(operation) { await runDuplicateAction(operation); return }
        let ids = selection
        await model.perform {
            switch operation {
            case "merge":
                let response: FaceMergeResponse = try await model.api.call("faces/groups/merge", FaceMergeRequest(faceGroupIds: Array(ids)))
                faces.removeAll { ids.contains($0.id) }; faces.append(response.group)
            case "reject":
                let _: RejectFacesResponse = try await model.api.call("faces/reject", RejectFacesRequest(requestId: UUID().uuidString, groupIds: Array(ids), faceGroupId: nil, faceIds: []))
                faces.removeAll { ids.contains($0.id) }
            case "empty":
                let _: MessageResponse = try await model.api.call("trash/empty", EmptyRequest()); trash.removeAll()
            case "restore", "delete":
                let _: MessageResponse = try await model.api.call("trash/\(operation)", MediaIdsRequest(mediaIds: Array(ids))); trash.removeAll { ids.contains($0.id) }
                if operation == "restore" { model.trashedMedia.subtract(ids) }
            default:
                guard CollectionPolicy.keepsOriginalInEveryGroup(duplicates, selected: ids) else {
                    throw ClientError.message("Keep at least one item in every similar group before moving media to Trash.")
                }
                try await model.trash(ids)
                for i in duplicates.indices { duplicates[i].items.removeAll { ids.contains($0.id) } }
                duplicates.removeAll { $0.items.count < 2 }
            }
            selection.removeAll(); action = nil
        }
    }
}
