use super::*;
use crate::audio::{AudioConfig32, AudioContext, AudioError32, PcmFormat32, SampleEncoding32};

mod numerics;
#[cfg(not(target_arch = "wasm32"))]
mod performance;
mod reverb;

#[test]
fn all_standard_parameters_validate_and_nonfinite_updates_are_rejected() {
    for kind in EffectKind::ALL {
        let defaults = kind.defaults();
        assert_eq!(
            Parameters::parse(kind, defaults.words(), 48_000).unwrap(),
            (defaults, false)
        );
        let mut bad = defaults.words().to_vec();
        bad[0] = f32::NAN.to_bits();
        assert!(Parameters::parse(kind, &bad, 48_000).is_err());
    }
    let mut eq = EffectKind::ParametricEq.defaults().words().to_vec();
    eq[0] = 16_000.0f32.to_bits();
    let (clamped, changed) = Parameters::parse(EffectKind::ParametricEq, &eq, 12_000).unwrap();
    assert!(changed);
    assert_eq!(clamped.words()[0], 4_000.0f32.to_bits());
}

#[test]
fn echo_impulse_is_literal_and_history_survives_processing_boundaries() {
    let mut chain = EffectChain::prepare(&[EffectKind::Echo], 1000, 32_000).unwrap();
    let values = [
        100.0f32.to_bits(),
        50.0f32.to_bits(),
        2.0f32.to_bits(),
        3.0f32.to_bits(),
        0,
    ];
    let parameters = Parameters::parse(EffectKind::Echo, &values, 1000)
        .unwrap()
        .0;
    chain.set_parameters(0, parameters).unwrap();
    assert_eq!(chain.process([1., 0.]), [0., 0.]);
    assert_eq!(chain.process([0.; 2]), [0.; 2]);
    assert_eq!(chain.process([0.; 2]), [1., 0.]);
    assert_eq!(chain.process([0.; 2]), [0.; 2]);
    assert_eq!(chain.process([0.; 2]), [0.5, 0.]);
}

#[test]
fn every_effect_processes_finite_pcm_and_chain_order_is_observable() {
    for kind in EffectKind::ALL {
        let mut chain = EffectChain::prepare(&[kind], 48_000, 4_000_000).unwrap();
        let mut difference = false;
        for frame in 0..4096 {
            let input = if frame == 0 { [0.75, -0.5] } else { [0.; 2] };
            let output = chain.process(input);
            assert!(output.iter().all(|value| value.is_finite()), "{kind:?}");
            difference |= output != input;
        }
        if kind != EffectKind::ParametricEq {
            assert!(difference, "{kind:?}");
        }
    }
    let mut first =
        EffectChain::prepare(&[EffectKind::Echo, EffectKind::Gargle], 1000, 64_000).unwrap();
    let mut second =
        EffectChain::prepare(&[EffectKind::Gargle, EffectKind::Echo], 1000, 64_000).unwrap();
    let echo = Parameters::parse(
        EffectKind::Echo,
        &[
            100.0f32.to_bits(),
            50.0f32.to_bits(),
            2.0f32.to_bits(),
            2.0f32.to_bits(),
            0,
        ],
        1000,
    )
    .unwrap()
    .0;
    first.set_parameters(0, echo).unwrap();
    second.set_parameters(1, echo).unwrap();
    let mut difference = false;
    for frame in 0..1024 {
        let input = if frame == 0 { [1.; 2] } else { [0.; 2] };
        difference |= first.process(input) != second.process(input);
    }
    assert!(difference);
}

#[test]
fn voice_effect_memory_is_bounded_reclaimed_and_submitted_pcm_is_immutable() {
    let mut audio = AudioContext::new(
        1,
        AudioConfig32 {
            sample_rate: 1000,
            max_buffer_bytes: 32_000,
            render_frames: 8,
            ..Default::default()
        },
    )
    .unwrap();
    let buffer = audio
        .create_stream_buffer(
            PcmFormat32 {
                sample_rate: 1000,
                channels: 1,
                bits_per_sample: 8,
            },
            SampleEncoding32::Integer,
            8,
        )
        .unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[255; 8], &[])
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    let chain = audio
        .prepare_voice_effects(voice, &[EffectKind::Gargle])
        .unwrap();
    audio.commit_voice_effects(voice, chain);
    audio.play_stream(voice, false).unwrap();
    assert!(matches!(
        audio.prepare_voice_effects(voice, &[EffectKind::Echo]),
        Err(AudioError32::Busy)
    ));
    let packet = audio.submit(8).unwrap();
    let before = audio.submission_pcm(packet).unwrap().to_vec();
    let changed = Parameters::parse(EffectKind::Gargle, &[1000, 1], 1000)
        .unwrap()
        .0;
    audio.set_effect_parameters(voice, 0, changed).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap(), before);
    audio.acknowledge(packet, 8).unwrap();
    audio.release_stream_voice(voice).unwrap();
    assert_eq!(audio.effect_bytes, 0);
    assert!(matches!(
        EffectChain::prepare(&[EffectKind::Echo], 1000, 1),
        Err(AudioError32::Capacity)
    ));
}
