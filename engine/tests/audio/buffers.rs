use super::{context, pcm};
use ring3_engine::audio::{AudioConfig32, AudioContext, AudioError32, PcmFormat32};

#[test]
fn literal_pcm8_and_pcm16_mono_stereo_samples_have_exact_stereo_output() {
    let cases: [(PcmFormat32, &[u8], &[f32]); 4] = [
        (
            pcm(1, 8),
            &[0, 64, 128, 192, 255],
            &[
                -1.0, -1.0, -0.5, -0.5, 0.0, 0.0, 0.5, 0.5, 0.9921875, 0.9921875,
            ],
        ),
        (pcm(2, 8), &[128, 0, 192, 255], &[0.0, -1.0, 0.5, 0.9921875]),
        (
            pcm(1, 16),
            &[0, 128, 0, 192, 0, 0, 0, 64, 255, 127],
            &[
                -1.0, -1.0, -0.5, -0.5, 0.0, 0.0, 0.5, 0.5, 0.9999695, 0.9999695,
            ],
        ),
        (
            pcm(2, 16),
            &[0, 128, 255, 127, 0, 192, 0, 64, 0, 0, 255, 255],
            &[-1.0, 0.9999695, -0.5, 0.5, 0.0, -0.000030517578],
        ),
    ];
    for (format, bytes, expected) in cases {
        let mut audio = context();
        let buffer = audio.create_buffer(format, bytes.len() as u32).unwrap();
        audio.write_buffer(buffer, 0, bytes).unwrap();
        let voice = audio.create_voice(buffer).unwrap();
        audio.play(voice, false).unwrap();
        let submission = audio.submit(expected.len() as u32 / 2).unwrap();
        assert_eq!(audio.submission_pcm(submission).unwrap(), expected);
        assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
        audio.acknowledge(submission, submission.frames).unwrap();
        assert!(!audio.voice_state(voice).unwrap().playing);
    }
}

#[test]
fn new_buffers_are_silent_in_both_unsigned_and_signed_formats() {
    for format in [pcm(1, 8), pcm(2, 8), pcm(1, 16), pcm(2, 16)] {
        let mut audio = context();
        let buffer = audio.create_buffer(format, 16).unwrap();
        let voice = audio.create_voice(buffer).unwrap();
        audio.play(voice, true).unwrap();
        let block = audio.submit(8).unwrap();
        assert_eq!(audio.submission_pcm(block).unwrap(), &[0.0; 16]);
    }
}

#[test]
fn wrapped_publication_and_rejected_partial_frames_preserve_literal_pcm() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(2, 16), 12).unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    audio
        .write_buffer(buffer, 8, &[0, 64, 0, 192, 255, 127, 0, 128])
        .unwrap();
    let before = (audio.state(), audio.voice_state(voice).unwrap());
    for (offset, bytes) in [
        (9, &[1; 8][..]),
        (12, &[1; 4][..]),
        (0, &[1; 16][..]),
        (0, &[1; 3][..]),
        (0, &[][..]),
    ] {
        assert_eq!(
            audio.write_buffer(buffer, offset, bytes),
            Err(AudioError32::InvalidRequest)
        );
        assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    }
    audio.play(voice, false).unwrap();
    let block = audio.submit(3).unwrap();
    assert_eq!(
        audio.submission_pcm(block).unwrap(),
        &[0.9999695, -1.0, 0.0, 0.0, 0.5, -0.5]
    );
}

#[test]
fn unsupported_formats_and_capacity_fail_before_slot_or_budget_publication() {
    let mut audio = context();
    let before = audio.state();
    for format in [
        PcmFormat32 {
            sample_rate: 44100,
            ..pcm(1, 16)
        },
        pcm(0, 16),
        pcm(3, 16),
        pcm(1, 24),
    ] {
        assert_eq!(
            audio.create_buffer(format, 8),
            Err(AudioError32::UnsupportedFormat)
        );
        assert_eq!(audio.state(), before);
    }
    assert_eq!(
        audio.create_buffer(pcm(2, 16), 3),
        Err(AudioError32::InvalidRequest)
    );
    assert_eq!(
        audio.create_buffer(pcm(1, 16), 0),
        Err(AudioError32::InvalidRequest)
    );
    assert_eq!(
        audio.create_buffer(pcm(1, 16), 130),
        Err(AudioError32::Capacity)
    );
    let full = audio.create_buffer(pcm(1, 16), 128).unwrap();
    assert_eq!((full.slot, full.generation), (0, 1));
    assert_eq!(
        audio.create_buffer(pcm(1, 8), 1),
        Err(AudioError32::Capacity)
    );
    audio.release_buffer(full).unwrap();
    assert!(audio.create_buffer(pcm(1, 16), 128).is_ok());
}

#[test]
fn provisioning_limits_reject_unbounded_eager_allocation_requests() {
    for config in [
        AudioConfig32 {
            sample_rate: 0,
            ..AudioConfig32::default()
        },
        AudioConfig32 {
            max_buffers: u32::MAX,
            ..AudioConfig32::default()
        },
        AudioConfig32 {
            max_voices: u32::MAX,
            ..AudioConfig32::default()
        },
        AudioConfig32 {
            max_buffer_bytes: u32::MAX,
            ..AudioConfig32::default()
        },
        AudioConfig32 {
            render_frames: u32::MAX,
            ..AudioConfig32::default()
        },
    ] {
        assert!(matches!(
            AudioContext::new(1, config),
            Err(AudioError32::InvalidConfig)
        ));
    }
    assert!(matches!(
        AudioContext::new(0, AudioConfig32::default()),
        Err(AudioError32::InvalidConfig)
    ));
}
