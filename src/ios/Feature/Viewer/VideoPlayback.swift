import AVFoundation
import UIKit
import Observation

@MainActor private final class ViewerAudioSession {
    static let shared = ViewerAudioSession()
    private var owner: UUID?
    func activate(owner: UUID) throws {
        let audio = AVAudioSession.sharedInstance()
        try audio.setCategory(.playback, mode: .moviePlayback)
        try audio.setActive(true)
        self.owner = owner
    }
    func release(owner: UUID) {
        guard self.owner == owner else { return }
        self.owner = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}

@MainActor @Observable final class VideoPlayback {
    let player = AVPlayer()
    private(set) var mediaID: Int64?
    var poster: UIImage?
    private(set) var position = 0.0
    private(set) var duration = 0.0
    private(set) var playing = false
    private(set) var muted = false
    private(set) var frameReady = false
    private(set) var error: String?
    private(set) var previewPosition: Double?
    @ObservationIgnored private weak var layer: AVPlayerLayer?
    @ObservationIgnored private var loader: AuthenticatedAssetLoader?
    @ObservationIgnored private let audioOwner = UUID()
    @ObservationIgnored private var primed = false
    @ObservationIgnored private var seekRevision = 0
    @ObservationIgnored private var seeking = false

    var displayedPosition: Double { previewPosition ?? position }
    var presentationState: String { frameReady ? "Ready" : poster != nil ? "Poster" : "Loading" }

    func load(media: Media, api: APIClient) {
        let loader = AuthenticatedAssetLoader(api: api, mediaId: media.id)
        prepare(asset: loader.asset(mediaId: media.id), mediaID: media.id, durationHint: media.durationSeconds ?? 0)
        self.loader = loader
    }
    func prepare(asset: AVAsset, mediaID: Int64, durationHint: Double) {
        stop()
        self.mediaID = mediaID
        duration = Self.validSeconds(durationHint)
        player.replaceCurrentItem(with: AVPlayerItem(asset: asset))
    }
    func attach(_ layer: AVPlayerLayer) { self.layer = layer }
    func synchronize() {
        guard let item = player.currentItem else { return }
        if item.status == .failed { error = item.error?.localizedDescription ?? "This video could not be played." }
        if item.status == .readyToPlay {
            let loadedDuration = Self.validSeconds(item.duration.seconds)
            if loadedDuration > 0 { duration = loadedDuration }
            if !primed {
                primed = true
                // Decode the first frame while paused; the poster stays visible until the layer is ready.
                if player.rate == 0 && !seeking { player.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero) }
            }
        }
        if previewPosition == nil && !seeking { position = Self.validSeconds(player.currentTime().seconds) }
        playing = player.rate > 0
        muted = player.isMuted
        frameReady = layer?.isReadyForDisplay == true
    }
    func play() {
        do {
            try ViewerAudioSession.shared.activate(owner: audioOwner)
            error = nil
            if duration > 0 && position >= duration - 0.1 { seek(to: 0) }
            player.play()
            playing = true
        } catch { self.error = error.localizedDescription }
    }
    func pause() {
        player.pause()
        playing = false
        ViewerAudioSession.shared.release(owner: audioOwner)
    }
    func toggleMute() { player.isMuted.toggle(); muted = player.isMuted }
    func previewSeek(to seconds: Double) { previewPosition = min(max(0, seconds), duration) }
    func finishSeeking() {
        guard let previewPosition else { return }
        self.previewPosition = nil
        seek(to: previewPosition)
    }
    func seek(to seconds: Double) {
        let target = min(max(0, seconds), duration)
        position = target
        seeking = true
        seekRevision += 1
        let revision = seekRevision
        player.seek(to: CMTime(seconds: target, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.seekRevision == revision else { return }
                self.seeking = false
                self.synchronize()
            }
        }
    }
    func stop() {
        pause()
        seekRevision += 1
        player.currentItem?.cancelPendingSeeks()
        player.replaceCurrentItem(with: nil)
        loader?.cancel(); loader = nil
        mediaID = nil; poster = nil; position = 0; duration = 0; frameReady = false
        error = nil; previewPosition = nil; seeking = false; primed = false
    }
    static func validSeconds(_ value: Double) -> Double { value.isFinite && value > 0 ? value : 0 }
    static func timeLabel(_ seconds: Double) -> String {
        let total = Int(min(validSeconds(seconds), Double(Int32.max)))
        return total >= 3600
            ? String(format: "%d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
            : String(format: "%d:%02d", total / 60, total % 60)
    }
}
