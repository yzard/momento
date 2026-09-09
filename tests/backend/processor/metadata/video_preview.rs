use super::*;

#[test]
fn preview_reservation_tracks_encoded_source_size_with_muxing_headroom() {
    assert!(output_limit(0).is_err());
    assert_eq!(
        output_limit(10_000_000).unwrap(),
        11_000_000 + 16 * 1024 * 1024
    );
    assert_eq!(
        output_limit(32 * 1024 * 1024 * 1024).unwrap(),
        32 * 1024 * 1024 * 1024
    );
    assert!(output_limit(u64::MAX).is_err());
}

#[test]
fn content_probe_selects_only_the_streams_that_need_transcoding() {
    let mut probe = crate::executor::ParsedFfprobeMetadata {
        payload_json: String::new(),
        width: Some(64),
        height: Some(32),
        video_codec: Some("h264".into()),
        video_profile: Some("High".into()),
        pixel_format: Some("yuv420p".into()),
        audio_present: true,
        audio_stream_ordinal: Some(0),
        audio_stream_count: 1,
        audio_codec: Some("aac".into()),
        major_brand: Some("isom".into()),
        duration_seconds: Some(2.0),
        date_taken: None,
        gps_latitude: None,
        gps_longitude: None,
    };
    assert!(!PreviewPlan::from_probe(&probe).unwrap().required);
    probe.audio_stream_count = 2;
    probe.audio_stream_ordinal = Some(1);
    let selected = PreviewPlan::from_probe(&probe).unwrap();
    assert!(selected.required && !selected.transcode_audio);
    assert_eq!(selected.audio_stream_ordinal, Some(1));
    probe.audio_stream_count = 1;
    probe.audio_stream_ordinal = Some(0);
    probe.pixel_format = Some("yuvj420p".into());
    assert!(!PreviewPlan::from_probe(&probe).unwrap().required);
    probe.major_brand = Some("qt  ".into());
    let remux = PreviewPlan::from_probe(&probe).unwrap();
    assert!(remux.required && !remux.transcode_video && !remux.transcode_audio);
    assert_eq!(
        remux.output_limit(1000).unwrap(),
        output_limit(1000).unwrap()
    );
    probe.audio_codec = Some("pcm_s16le".into());
    let audio = PreviewPlan::from_probe(&probe).unwrap();
    assert!(!audio.transcode_video && audio.transcode_audio);
    assert!(audio.output_limit(1000).unwrap() > remux.output_limit(1000).unwrap());
    for (profile, pixels) in [
        ("Main", "yuv420p"),
        ("Main", "yuvj420p"),
        ("Main 10", "yuvj420p"),
        ("Main 10", "yuv420p10le"),
    ] {
        probe.video_codec = Some("hevc".into());
        probe.video_profile = Some(profile.into());
        probe.pixel_format = Some(pixels.into());
        let plan = PreviewPlan::from_probe(&probe).unwrap();
        assert!(plan.copy_hevc && !plan.transcode_video && plan.transcode_audio);
        assert_eq!(
            plan.output_limit(1000).unwrap(),
            audio.output_limit(1000).unwrap()
        );
        probe.audio_codec = Some("aac".into());
        probe.major_brand = Some("isom".into());
        assert!(!PreviewPlan::from_probe(&probe).unwrap().required);
        probe.audio_codec = Some("pcm_s16le".into());
        probe.major_brand = Some("qt  ".into());
    }
    for (codec, profile, pixels) in [
        ("hevc", "Rext", "yuv444p12le"),
        ("h264", "High 10", "yuv420p10le"),
        ("h264", "High 4:4:4 Predictive", "yuv444p"),
    ] {
        probe.video_codec = Some(codec.into());
        probe.video_profile = Some(profile.into());
        probe.pixel_format = Some(pixels.into());
        assert!(PreviewPlan::from_probe(&probe).unwrap().transcode_video);
    }
    probe.audio_present = false;
    probe.audio_stream_ordinal = None;
    probe.audio_stream_count = 0;
    assert!(!PreviewPlan::from_probe(&probe).unwrap().transcode_audio);
    probe.duration_seconds = None;
    assert!(PreviewPlan::from_probe(&probe)
        .unwrap()
        .output_limit(1000)
        .is_err());
    probe.width = Some(63);
    assert!(PreviewPlan::from_probe(&probe).is_err());
    probe.video_codec = None;
    assert!(PreviewPlan::from_probe(&probe).is_err());
}
