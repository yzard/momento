import SwiftUI

struct AlbumEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let album: AlbumDetail?
    let mediaIds: [Int64]
    let saved: (AlbumDetail) -> Void
    @State private var name = ""
    @State private var description = ""
    @State private var busy = false
    @State private var discard = false
    private var dirty: Bool { name != (album?.name ?? "") || description != (album?.description ?? "") }
    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                TextField("Description", text: $description, axis: .vertical)
            }.navigationTitle(album == nil ? "New album" : "Edit album")
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("Cancel") { if dirty { discard = true } else { dismiss() } } }
                    ToolbarItem(placement: .confirmationAction) { Button("Save") { Task { await save() } }.disabled(busy || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
                }
        }.onAppear { name = album?.name ?? ""; description = album?.description ?? "" }
            .interactiveDismissDisabled(dirty || busy)
            .confirmationDialog("Discard changes?", isPresented: $discard) { Button("Discard", role: .destructive) { dismiss() } }
    }
    private func save() async {
        busy = true; defer { busy = false }
        await model.perform {
            if var detail = album {
                let result: Album = try await model.api.call("album/update", AlbumUpdateRequest(albumId: detail.id, name: name.trimmingCharacters(in: .whitespacesAndNewlines), description: description, coverMediaId: nil))
                detail.name = result.name; detail.description = result.description; detail.coverMediaId = result.coverMediaId
                model.publishAlbum(detail)
                saved(detail)
            } else {
                let result: AlbumDetail = try await model.api.call("album/create", AlbumCreateRequest(name: name.trimmingCharacters(in: .whitespacesAndNewlines), description: description.isEmpty ? nil : description, mediaIds: mediaIds))
                model.publishAlbum(result); saved(result)
            }
            dismiss()
        }
    }
}

struct AlbumPicker: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let mediaIds: [Int64]
    @State private var albums: [Album] = []
    @State private var creating = false
    var body: some View {
        NavigationStack {
            List {
                Button("Create album", systemImage: "plus") { creating = true }
                ForEach(albums) { album in
                    Button(album.name) { Task { await model.perform {
                        let _: MessageResponse = try await model.api.call("album/add-media", AlbumMediaRequest(albumId: album.id, mediaIds: mediaIds))
                        let detail: AlbumDetail = try await model.api.call("album/get", AlbumIdRequest(albumId: album.id))
                        model.publishAlbum(detail); dismiss()
                    } } }
                }
            }.navigationTitle("Add \(mediaIds.count) items")
                .toolbar { Button("Cancel") { dismiss() } }
                .task { await model.perform { let result: AlbumsResponse = try await model.api.call("album/list", EmptyRequest()); albums = result.albums } }
                .sheet(isPresented: $creating) { AlbumEditor(album: nil, mediaIds: mediaIds) { _ in dismiss() } }
        }
    }
}
