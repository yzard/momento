import SwiftUI

struct CollectionDetail: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let target: CollectionTarget
    let updatedAlbum: (Album) -> Void
    let deletedAlbum: (Int64) -> Void
    @State private var album: AlbumDetail?
    @State private var media: [Media] = []
    @State private var detections: [FaceDetection] = []
    @State private var selectedFaces: Set<Int64> = []
    @State private var selection: Set<Int64> = []
    @State private var selecting = false
    @State private var viewer: Media?
    @State private var editing = false
    @State private var next: String?
    @State private var hasMore = false
    @State private var loaded = false
    @State private var error: String?
    @State private var action: String?
    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading) {
                    if target.destination == .faces {
                        Text("Select incorrect face detections to reject").font(.caption).padding()
                        ScrollView(.horizontal) {
                            HStack {
                                ForEach(detections) { face in
                                    Button { if !selectedFaces.insert(face.id).inserted { selectedFaces.remove(face.id) } } label: {
                                        RemoteImage(path: "faces/detections/\(face.id)/thumbnail").frame(width: 72, height: 72)
                                            .overlay(alignment: .topLeading) { SelectionMark(selected: selectedFaces.contains(face.id)).padding(4) }
                                    }
                                }
                            }.padding()
                        }
                        Button("Reject \(selectedFaces.count) faces") { action = "reject" }.disabled(selectedFaces.isEmpty).padding(.horizontal)
                    }
                    MediaGrid(media: media, selection: $selection, selecting: selecting, visibilityChanged: nil) { viewer = $0 }
                    if hasMore { Button("Load more") { Task { await load() } } }
                    if let error { FailureView(message: error) { Task { await load() } } }
                    if !loaded && error == nil { ProgressView().frame(maxWidth: .infinity).padding() }
                    if loaded && media.isEmpty { ContentUnavailableView("No media", systemImage: "photo") }
                }.padding(6)
            }
            .scrollEdgeEffectHidden(true, for: .top)
            .toolbarBackground(.hidden, for: .navigationBar)
            .navigationTitle(album?.name ?? target.title)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() } }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    if target.destination == .albums {
                        Menu {
                            Button("Edit album") { editing = true }
                            Button(selecting ? "Finish selection" : "Select media") { selecting.toggle() }
                            Button("Delete album", role: .destructive) { action = "delete" }
                        } label: { Image(systemName: "ellipsis") }
                    }
                }
            }
            .safeAreaBar(edge: .bottom) {
                if selecting {
                    HStack {
                        Button("Remove", systemImage: "minus.circle") { action = "remove" }.disabled(selection.isEmpty)
                        Button("Earlier", systemImage: "arrow.left") { Task { await reorder(delta: -1) } }.disabled(selection.count != 1)
                        Button("Later", systemImage: "arrow.right") { Task { await reorder(delta: 1) } }.disabled(selection.count != 1)
                        Button("Set cover", systemImage: "photo.badge.checkmark") { Task { await cover() } }.disabled(selection.count != 1)
                    }.labelStyle(.iconOnly).buttonStyle(.glass).padding().glassEffect(in: .capsule).padding()
                }
            }
            .task { if !loaded { await load() } }
            .sheet(isPresented: $editing) { if let album { AlbumEditor(album: album, mediaIds: []) { detail in self.album = detail; publishAlbum() } } }
            .fullScreenCover(item: $viewer) { item in MediaViewer(media: media, initial: item.id, removed: { ids in media.removeAll { ids.contains($0.id) }; publishAlbum() }) }
            .confirmationDialog("Confirm \(action ?? "action")?", isPresented: Binding(get: { action != nil }, set: { if !$0 { action = nil } }), titleVisibility: .visible) {
                if let action { Button(action.capitalized, role: .destructive) { Task { await mutate(action) } } }
            }
        }
    }
    private func load() async {
        do {
            switch target.destination {
            case .albums:
                let result: AlbumDetail = try await model.api.call("album/get", AlbumIdRequest(albumId: target.numericId)); album = result; media = result.media
            case .faces:
                let result: FaceGroupMediaResponse = try await model.api.call("faces/groups/get", FaceGroupRequest(faceGroupId: target.numericId)); media = result.media; detections = result.faces
            default:
                let result: PlaceResponse = try await model.api.call("places/get", PlaceRequest(placeId: target.stringId, cursor: next, limit: 100)); media += result.media; next = result.nextCursor; hasMore = result.hasMore
            }
            loaded = true; error = nil
        } catch { self.error = error.localizedDescription }
    }
    private func publishAlbum() {
        guard var album else { return }
        album.media = media; self.album = album
        model.publishAlbum(album)
        updatedAlbum(Album(id: album.id, name: album.name, description: album.description, coverMediaId: album.coverMediaId,
            thumbnailMediaIds: Array(media.prefix(4).map(\.id)), mediaCount: Int64(media.count), createdAt: album.createdAt))
    }
    private func mutate(_ operation: String) async {
        await model.perform {
            if operation == "delete" {
                let _: MessageResponse = try await model.api.call("album/delete", AlbumIdRequest(albumId: target.numericId))
                model.deletedAlbums.insert(target.numericId); model.albumUpdates[target.numericId] = nil
                deletedAlbum(target.numericId); dismiss()
            } else if operation == "reject" {
                let _: RejectFacesResponse = try await model.api.call("faces/reject", RejectFacesRequest(requestId: UUID().uuidString, groupIds: [], faceGroupId: target.numericId, faceIds: Array(selectedFaces)))
                detections.removeAll { selectedFaces.contains($0.id) }; selectedFaces.removeAll()
                let remainingMedia = Set(detections.map(\.mediaId)); media.removeAll { !remainingMedia.contains($0.id) }
            } else {
                let _: MessageResponse = try await model.api.call("album/remove-media", AlbumMediaRequest(albumId: target.numericId, mediaIds: Array(selection)))
                media.removeAll { selection.contains($0.id) }; selection.removeAll(); publishAlbum()
            }
            action = nil
        }
    }
    private func reorder(delta: Int) async {
        guard let id = selection.first, let index = media.firstIndex(where: { $0.id == id }), media.indices.contains(index + delta) else { return }
        var updated = media; updated.swapAt(index, index + delta)
        await model.perform {
            let _: MessageResponse = try await model.api.call("album/reorder", AlbumMediaRequest(albumId: target.numericId, mediaIds: updated.map(\.id)))
            media = updated; publishAlbum()
        }
    }
    private func cover() async {
        guard let id = selection.first else { return }
        await model.perform {
            let result: Album = try await model.api.call("album/update", AlbumUpdateRequest(albumId: target.numericId, name: nil, description: nil, coverMediaId: id))
            album?.coverMediaId = result.coverMediaId; updatedAlbum(result)
        }
    }
}
