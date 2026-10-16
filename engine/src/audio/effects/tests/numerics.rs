use super::*;
use crate::audio::{AudioVoiceControls32, AudioVoiceId32};

fn compressor(gain: f32) -> Parameters {
    Parameters::parse(
        EffectKind::Compressor,
        &[
            gain.to_bits(),
            10.0f32.to_bits(),
            200.0f32.to_bits(),
            0.0f32.to_bits(),
            1.0f32.to_bits(),
            0.0f32.to_bits(),
        ],
        1000,
    )
    .unwrap()
    .0
}
fn voice(audio: &mut AudioContext, bytes: &[u8], count: usize) -> AudioVoiceId32 {
    let buffer = audio
        .create_stream_buffer(
            PcmFormat32 {
                sample_rate: 500,
                channels: 1,
                bits_per_sample: 8,
            },
            SampleEncoding32::Integer,
            bytes.len() as u32,
        )
        .unwrap();
    audio.write_stream_buffer(buffer, 0, bytes, &[]).unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    let mut chain = audio
        .prepare_voice_effects(voice, &vec![EffectKind::Compressor; count])
        .unwrap();
    for index in 0..count {
        chain.set_parameters(index, compressor(60.)).unwrap();
    }
    audio.commit_voice_effects(voice, chain);
    audio
        .set_voice_controls(
            voice,
            AudioVoiceControls32 {
                frequency: 500,
                gains: [16.; 2],
            },
        )
        .unwrap();
    audio.play_stream(voice, true).unwrap();
    voice
}

#[test]
fn maximum_admitted_compressor_chain_saturates_only_at_float_storage_boundaries() {
    for count in [13, EffectChain::MAX_EFFECTS] {
        let mut chain =
            EffectChain::prepare(&vec![EffectKind::Compressor; count], 1000, 1 << 20).unwrap();
        for index in 0..count {
            chain.set_parameters(index, compressor(60.)).unwrap();
        }
        assert_eq!(chain.process([1., -1.]), [f32::MAX, -f32::MAX]);
        assert_eq!(chain.process([f32::MAX, -f32::MAX]), [f32::MAX, -f32::MAX]);
        assert_eq!(chain.process([0.; 2]), [0.; 2]);
    }
    assert!(matches!(
        EffectChain::prepare(&[EffectKind::Compressor; 33], 1000, 1 << 20),
        Err(AudioError32::InvalidRequest)
    ));
    let mut chain = EffectChain::prepare(&[EffectKind::Compressor], 1000, 1 << 20).unwrap();
    chain.set_parameters(0, compressor(20.)).unwrap();
    assert_eq!(chain.process([0.25, -0.25]), [2.5, -2.5]);
}

#[test]
fn integer_fx_voices_mix_finitely_with_gain_interpolation_and_exact_cancellation() {
    let mut audio = AudioContext::new(
        1,
        AudioConfig32 {
            sample_rate: 1000,
            render_frames: 4,
            ..Default::default()
        },
    )
    .unwrap();
    voice(&mut audio, &[255, 1], EffectChain::MAX_EFFECTS);
    let first = audio.submit(4).unwrap();
    assert_eq!(
        audio.submission_pcm(first).unwrap(),
        &[1., 1., 0., 0., -1., -1., 0., 0.]
    );
    let storage = (audio.wide_output.as_ptr(), audio.wide_output.capacity());
    audio.acknowledge(first, 4).unwrap();
    voice(&mut audio, &[1, 255], EffectChain::MAX_EFFECTS);
    for _ in 0..4 {
        let packet = audio.submit(4).unwrap();
        assert_eq!(audio.submission_pcm(packet).unwrap(), &[0.; 8]);
        assert_eq!(
            (audio.wide_output.as_ptr(), audio.wide_output.capacity()),
            storage
        );
        audio.acknowledge(packet, 4).unwrap();
    }
}

#[test]
fn floating_fx_headroom_survives_until_voice_gain_and_final_output() {
    let mut audio = AudioContext::new(
        1,
        AudioConfig32 {
            sample_rate: 1000,
            render_frames: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let input = [2.0f32.to_le_bytes(); 2].concat();
    let buffer = audio
        .create_stream_buffer(
            PcmFormat32 {
                sample_rate: 1000,
                channels: 1,
                bits_per_sample: 32,
            },
            SampleEncoding32::Float,
            input.len() as u32,
        )
        .unwrap();
    audio.write_stream_buffer(buffer, 0, &input, &[]).unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    let chain = audio
        .prepare_voice_effects(voice, &[EffectKind::ParametricEq])
        .unwrap();
    audio.commit_voice_effects(voice, chain);
    audio
        .set_voice_controls(
            voice,
            AudioVoiceControls32 {
                frequency: 1000,
                gains: [0.25, 0.125],
            },
        )
        .unwrap();
    audio.play_stream(voice, false).unwrap();
    let packet = audio.submit(2).unwrap();
    assert_eq!(
        audio.submission_pcm(packet).unwrap(),
        &[0.5, 0.25, 0.5, 0.25]
    );
    assert_eq!(audio.stream_bytes(buffer).unwrap(), input);
}
