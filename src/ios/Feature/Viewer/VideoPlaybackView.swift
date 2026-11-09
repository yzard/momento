import AVFoundation
import SwiftUI
import UIKit

private final class VideoLayerView: UIView {
    override class var layerClass: AnyClass { AVPlayerLayer.self }
    var videoLayer: AVPlayerLayer { layer as! AVPlayerLayer }
}

private struct VideoSurface: UIViewRepresentable {
    let playback: VideoPlayback
    func makeUIView(context: Context) -> VideoLayerView {
        let view = VideoLayerView()
        view.backgroundColor = .black
        view.isAccessibilityElement = true
        view.accessibilityIdentifier = "videoPlayer"
        view.accessibilityLabel = "Video"
        view.videoLayer.videoGravity = .resizeAspect
        updateUIView(view, context: context)
        return view
    }
    func updateUIView(_ view: VideoLayerView, context: Context) {
        if view.videoLayer.player !== playback.player { view.videoLayer.player = playback.player }
        playback.attach(view.videoLayer)
        view.accessibilityValue = playback.presentationState
    }
    static func dismantleUIView(_ view: VideoLayerView, coordinator: ()) { view.videoLayer.player = nil }
}

struct VideoPlaybackView: View {
    @Bindable var playback: VideoPlayback
    let controls: Bool
    let toggleControls: () -> Void
    var body: some View {
        ZStack(alignment: .bottom) {
            ZStack {
                VideoSurface(playback: playback)
                if !playback.frameReady {
                    if let poster = playback.poster {
                        Image(uiImage: poster).resizable().scaledToFit()
                            .accessibilityLabel("Video preview").accessibilityIdentifier("videoPoster")
                    } else { ProgressView().tint(.white) }
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(.rect).onTapGesture(perform: toggleControls)
            if controls {
                HStack(spacing: 8) {
                    Button {
                        if playback.playing { playback.pause() } else { playback.play() }
                    } label: {
                        Image(systemName: playback.playing ? "pause.fill" : "play.fill").frame(width: 40, height: 44).contentShape(.rect)
                    }.accessibilityLabel(playback.playing ? "Pause video" : "Play video")
                    VStack(spacing: 0) {
                        Slider(value: Binding(get: { playback.displayedPosition }, set: { playback.previewSeek(to: $0) }),
                               in: 0...max(1, playback.duration), onEditingChanged: { editing in if !editing { playback.finishSeeking() } })
                            .disabled(playback.duration <= 0).tint(.white).accessibilityLabel("Video position").accessibilityIdentifier("videoSeekBar")
                        HStack {
                            Text(VideoPlayback.timeLabel(playback.displayedPosition)).accessibilityIdentifier("videoElapsedTime")
                            Spacer()
                            Text(VideoPlayback.timeLabel(playback.duration)).accessibilityIdentifier("videoDuration")
                        }.font(.caption2.monospacedDigit())
                    }
                    Button { playback.toggleMute() } label: {
                        Image(systemName: playback.muted ? "speaker.slash.fill" : "speaker.wave.2.fill").frame(width: 40, height: 44).contentShape(.rect)
                    }.accessibilityLabel(playback.muted ? "Unmute video" : "Mute video")
                }.buttonStyle(.plain).foregroundStyle(.white).padding(.horizontal, 8).padding(.vertical, 6)
                    .glassEffect(.regular, in: .capsule).padding(.horizontal, 12).padding(.bottom, 8)
            }
            if let error = playback.error { Text(error).font(.callout).foregroundStyle(.white).padding().background(.black.opacity(0.8)).frame(maxHeight: .infinity) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
