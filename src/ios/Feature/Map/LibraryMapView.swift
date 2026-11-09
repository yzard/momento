import SwiftUI
import MapKit

struct LibraryMapView: View {
    @Environment(AppModel.self) private var model
    @State private var camera: MapCameraPosition = .automatic
    @State private var clusters: [MapCluster] = []
    @State private var bounds = BoundingBox(north: 85, south: -85, east: 180, west: -180)
    @State private var gallery: MediaPresentation?
    @State private var request: Task<Void, Never>?
    @State private var generation = UUID()
    var body: some View {
        Map(position: $camera) {
            ForEach(clusters) { cluster in
                Annotation("Media group", coordinate: CLLocationCoordinate2D(latitude: cluster.lat, longitude: cluster.lng)) {
                    Button { Task { await open(cluster) } } label: {
                        MapClusterMarker(count: cluster.count) {
                            RemoteImage(path: "media/\(cluster.representativeId)/thumbnail/tiny")
                        }
                    }.buttonStyle(.plain).accessibilityLabel("\(cluster.count) media")
                        .accessibilityIdentifier("mapCluster.\(cluster.id)")
                }.annotationTitles(.hidden)
            }
        }
        .mapControls { MapCompass(); MapScaleView() }
        .onMapCameraChange(frequency: .onEnd) { context in
            let region = context.region
            bounds = BoundingBox(north: min(90, region.center.latitude + region.span.latitudeDelta / 2), south: max(-90, region.center.latitude - region.span.latitudeDelta / 2),
                east: min(180, region.center.longitude + region.span.longitudeDelta / 2), west: max(-180, region.center.longitude - region.span.longitudeDelta / 2))
            let zoom = max(0, min(20, Int(log2(360 / max(region.span.longitudeDelta, 0.0001)))))
            let token = UUID(); generation = token; request?.cancel()
            request = Task { do {
                let result: MapClustersResponse = try await model.api.call("map/clusters", MapClustersRequest(bounds: bounds, zoom: zoom))
                if generation == token { clusters = result.clusters }
            } catch is CancellationError { } catch { if generation == token { model.error = error.localizedDescription } } }
        }
        .sheet(item: $gallery) { MapGallery(media: $0.media) }
    }
    private func open(_ cluster: MapCluster) async {
        await model.perform {
            let result: MapMediaResponse = try await model.api.call("map/media", MapMediaRequest(bounds: bounds, geohashPrefixes: [cluster.id]))
            gallery = MediaPresentation(media: result.items, initial: result.items.first?.id ?? 0)
        }
    }
}

private struct MapGallery: View {
    @Environment(\.dismiss) private var dismiss
    @State var media: [Media]
    @State private var selection: Set<Int64> = []
    @State private var viewer: Media?

    var body: some View {
        NavigationStack {
            ScrollView { MediaGrid(media: media, selection: $selection, selecting: false, visibilityChanged: nil) { viewer = $0 }.padding(6) }
                .navigationTitle("\(media.count) moments").toolbar { Button("Done") { dismiss() } }
                .fullScreenCover(item: $viewer) { item in
                    MediaViewer(media: media, initial: item.id, removed: { ids in media.removeAll { ids.contains($0.id) } })
                }
        }
    }
}
