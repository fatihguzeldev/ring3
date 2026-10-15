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
