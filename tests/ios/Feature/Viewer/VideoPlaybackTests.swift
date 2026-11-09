import AVFoundation
import Testing
@testable import Momento

private final class VideoFixtureBundle {}

@Suite(.serialized) @MainActor struct VideoPlaybackTests {
    @Test func fragmentedVideoPrimesAdvancesSeeksAndUsesPlaybackAudio() async throws {
        let url = try #require(Bundle(for: VideoFixtureBundle.self).url(forResource: "fragmented-preview", withExtension: "mp4", subdirectory: "media"))
        let playback = VideoPlayback()
        defer { playback.stop() }
        playback.prepare(asset: AVURLAsset(url: url), mediaID: 10915, durationHint: 10)
        let deadline = Date().addingTimeInterval(10)
        while playback.player.currentItem?.status != .readyToPlay && Date() < deadline {
            playback.synchronize(); try await Task.sleep(for: .milliseconds(50))
        }
        playback.synchronize()
        #expect(playback.player.currentItem?.status == .readyToPlay)
        #expect(playback.duration >= 9.9 && playback.duration < 10.5)
        playback.toggleMute()
        #expect(playback.muted && playback.player.isMuted)
        playback.play()
        #expect(AVAudioSession.sharedInstance().category == .playback)
        #expect(AVAudioSession.sharedInstance().mode == .moviePlayback)
        let progressDeadline = Date().addingTimeInterval(5)
        while playback.position < 0.6 && Date() < progressDeadline {
            try await Task.sleep(for: .milliseconds(100)); playback.synchronize()
        }
        #expect(playback.position > 0.5)
        playback.pause()
        playback.previewSeek(to: 6.5)
        #expect(playback.displayedPosition == 6.5)
        playback.finishSeeking()
        try await Task.sleep(for: .milliseconds(500)); playback.synchronize()
        #expect(abs(playback.position - 6.5) < 0.2)
        playback.toggleMute()
        #expect(!playback.player.isMuted)
        playback.stop()
        #expect(playback.mediaID == nil && playback.player.currentItem == nil)
    }
    @Test func invalidTimesNeverAppearInControls() {
        for value in [Double.nan, .infinity, -.infinity, -2] {
            #expect(VideoPlayback.validSeconds(value) == 0)
            #expect(VideoPlayback.timeLabel(value) == "0:00")
        }
        #expect(VideoPlayback.timeLabel(208.68) == "3:28")
        #expect(VideoPlayback.timeLabel(3661) == "1:01:01")
    }
}
