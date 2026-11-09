import AVFoundation
import UniformTypeIdentifiers

@MainActor final class AuthenticatedAssetLoader: NSObject, AVAssetResourceLoaderDelegate {
    private let api: APIClient
    private let path: String
    private var requests: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(api: APIClient, mediaId: Int64) {
        self.api = api
        path = "media/\(mediaId)/original"
    }

    func asset(mediaId: Int64) -> AVURLAsset {
        let asset = AVURLAsset(url: URL(string: "momento-media://original/\(mediaId)")!)
        asset.resourceLoader.setDelegate(self, queue: .main)
        return asset
    }

    nonisolated func resourceLoader(_ resourceLoader: AVAssetResourceLoader, shouldWaitForLoadingOfRequestedResource loadingRequest: AVAssetResourceLoadingRequest) -> Bool {
        MainActor.assumeIsolated {
            let key = ObjectIdentifier(loadingRequest)
            requests[key] = Task {
                defer { requests[key] = nil }
                do {
                    if let info = loadingRequest.contentInformationRequest {
                        let range = try await api.mediaRange(path: path, offset: 0, length: 2)
                        info.contentLength = range.total
                        info.contentType = UTType(mimeType: range.mimeType)?.identifier
                        info.isByteRangeAccessSupported = true
                    }
                    if let request = loadingRequest.dataRequest {
                        var offset = max(request.requestedOffset, request.currentOffset)
                        var end = request.requestsAllDataToEndOfResource ? Int64.max : request.requestedOffset + Int64(request.requestedLength)
                        while offset < end {
                            try Task.checkCancellation()
                            let range = try await api.mediaRange(path: path, offset: offset, length: Int(min(1024 * 1024, end - offset)))
                            try Task.checkCancellation()
                            request.respond(with: range.data)
                            offset += Int64(range.data.count)
                            end = min(end, range.total)
                        }
                    }
                    loadingRequest.finishLoading()
                } catch {
                    if !loadingRequest.isCancelled { loadingRequest.finishLoading(with: error) }
                }
            }
        }
        return true
    }

    nonisolated func resourceLoader(_ resourceLoader: AVAssetResourceLoader, didCancel loadingRequest: AVAssetResourceLoadingRequest) {
        MainActor.assumeIsolated { requests.removeValue(forKey: ObjectIdentifier(loadingRequest))?.cancel() }
    }
    func cancel() { for task in requests.values { task.cancel() }; requests.removeAll() }
}
