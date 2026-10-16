use super::*;

#[test]
fn half_rate_linear_resampling_and_independent_channel_gains_are_literal() {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(24_000), SampleEncoding32::Integer, 3)
        .unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[128, 192, 128], &[])
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio
        .set_voice_controls(
            voice,
            AudioVoiceControls32 {
                frequency: 24_000,
                gains: [1.0, 0.5],
            },
        )
        .unwrap();
    audio.play_stream(voice, false).unwrap();
    let packet = audio.submit(6).unwrap();
    assert!(audio.wide_output.is_empty());
    assert_eq!(
        audio.submission_pcm(packet).unwrap(),
        &[
            0.0, 0.0, 0.25, 0.125, 0.5, 0.25, 0.25, 0.125, 0.0, 0.0, 0.0, 0.0
        ]
    );
    audio.acknowledge(packet, 3).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 1);
    audio.acknowledge(packet, 6).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
    assert!(!audio.voice_state(voice).unwrap().playing);
}

#[test]
fn pcm24_signed_endpoints_and_float_nonfinite_samples_are_bounded() {
    for (encoding, bits, bytes, expected) in [
        (
            SampleEncoding32::Integer,
            24,
            vec![0, 0, 128, 0, 0, 64],
            [-1.0, 0.5],
        ),
        (
            SampleEncoding32::Float,
            32,
            [f32::NAN.to_le_bytes(), 0.5f32.to_le_bytes()].concat(),
            [0.0, 0.5],
        ),
    ] {
        let mut audio = context();
        let format = PcmFormat32 {
            bits_per_sample: bits,
            ..mono(48_000)
        };
        let buffer = audio
            .create_stream_buffer(format, encoding, bytes.len() as u32)
            .unwrap();
        audio.write_stream_buffer(buffer, 0, &bytes, &[]).unwrap();
        let voice = audio.create_stream_voice(buffer).unwrap();
        audio.play_stream(voice, false).unwrap();
        let packet = audio.submit(2).unwrap();
        assert_eq!(
            audio.submission_pcm(packet).unwrap(),
            &[expected[0], expected[0], expected[1], expected[1]]
        );
    }
}

#[test]
fn finite_float_extremes_resample_without_nan_and_keep_input_headroom() {
    let mut audio = context();
    let bytes = [f32::MAX.to_le_bytes(), (-f32::MAX).to_le_bytes()].concat();
    let buffer = audio
        .create_stream_buffer(
            PcmFormat32 {
                bits_per_sample: 32,
                ..mono(24_000)
            },
            SampleEncoding32::Float,
            bytes.len() as u32,
        )
        .unwrap();
    audio.write_stream_buffer(buffer, 0, &bytes, &[]).unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, false).unwrap();
    let packet = audio.submit(4).unwrap();
    assert_eq!(
        audio.submission_pcm(packet).unwrap(),
        &[1.0, 1.0, 0.0, 0.0, -1.0, -1.0, -1.0, -1.0]
    );
    assert_eq!(audio.stream_bytes(buffer).unwrap(), bytes);
    let scratch = audio.wide_output.as_ptr();
    let capacity = audio.wide_output.capacity();
    audio.acknowledge(packet, 4).unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[2.0f32.to_le_bytes(); 2].concat(), &[])
        .unwrap();
    audio
        .set_voice_controls(
            voice,
            AudioVoiceControls32 {
                frequency: 48_000,
                gains: [0.25, 0.0],
            },
        )
        .unwrap();
    audio.play_stream(voice, false).unwrap();
    let packet = audio.submit(2).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap(), &[0.5, 0.0, 0.5, 0.0]);
    assert_eq!(audio.wide_output.as_ptr(), scratch);
    assert_eq!(audio.wide_output.capacity(), capacity);
}

#[test]
fn offsetting_float_voices_and_muted_extremes_mix_before_final_clipping() {
    let mut audio = context();
    for value in [f32::MAX, f32::MAX, -f32::MAX, -f32::MAX] {
        let buffer = audio
            .create_stream_buffer(
                PcmFormat32 {
                    bits_per_sample: 32,
                    ..mono(48_000)
                },
                SampleEncoding32::Float,
                4,
            )
            .unwrap();
        audio
            .write_stream_buffer(buffer, 0, &value.to_le_bytes(), &[])
            .unwrap();
        let voice = audio.create_stream_voice(buffer).unwrap();
        audio
            .set_voice_controls(
                voice,
                AudioVoiceControls32 {
                    frequency: 48_000,
                    gains: [16.0, 0.0],
                },
            )
            .unwrap();
        audio.play_stream(voice, true).unwrap();
    }
    let packet = audio.submit(4).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap(), &[0.0; 8]);
}
