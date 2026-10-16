use super::*;
use std::time::Instant;

#[test]
#[ignore = "explicit release-mode software FX measurement"]
fn preallocated_effect_chains_report_steady_submission_cost() {
    assert!(
        !cfg!(debug_assertions),
        "run this measurement with --release"
    );
    let representative = [
        EffectKind::Chorus,
        EffectKind::Compressor,
        EffectKind::ParametricEq,
        EffectKind::I3dl2Reverb,
    ];
    let compressors = [EffectKind::Compressor; EffectChain::MAX_EFFECTS];
    for (profile, kinds) in [
        ("representative", representative.as_slice()),
        ("maximum_compressors", compressors.as_slice()),
    ] {
        const FRAMES: u32 = 480;
        const WARMUPS: usize = 100;
        const MEASUREMENTS: usize = 300;
        let input: Vec<u8> = (0..FRAMES)
            .flat_map(|frame| (((frame % 96) as i16 - 48) * 384).to_le_bytes())
            .collect();
        let mut audio = AudioContext::new(
            1,
            AudioConfig32 {
                sample_rate: 48_000,
                max_buffers: 1,
                max_voices: 1,
                max_buffer_bytes: 8 << 20,
                render_frames: FRAMES,
            },
        )
        .unwrap();
        let buffer = audio
            .create_stream_buffer(
                PcmFormat32 {
                    sample_rate: 48_000,
                    channels: 1,
                    bits_per_sample: 16,
                },
                SampleEncoding32::Integer,
                input.len() as u32,
            )
            .unwrap();
        audio.write_stream_buffer(buffer, 0, &input, &[]).unwrap();
        let voice = audio.create_stream_voice(buffer).unwrap();
        let chain = audio.prepare_voice_effects(voice, kinds).unwrap();
        audio.commit_voice_effects(voice, chain);
        audio.play_stream(voice, true).unwrap();
        for _ in 0..WARMUPS {
            let packet = audio.submit(FRAMES).unwrap();
            assert!(
                audio
                    .submission_pcm(packet)
                    .unwrap()
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.)
            );
            audio.acknowledge(packet, FRAMES).unwrap();
        }

        let chain = audio.voice_effects(voice).unwrap().unwrap();
        let effects_storage = (chain.effects.as_ptr(), chain.effects.capacity());
        let delay_storage: Vec<_> = chain
            .effects
            .iter()
            .map(|effect| {
                (
                    effect.delay.samples.as_ptr(),
                    effect.delay.samples.capacity(),
                )
            })
            .collect();
        let storage = (
            (audio.output.as_ptr(), audio.output.capacity()),
            (audio.wide_output.as_ptr(), audio.wide_output.capacity()),
            (audio.spans.as_ptr(), audio.spans.capacity()),
            (audio.voices.as_ptr(), audio.voices.capacity()),
            (audio.buffers.as_ptr(), audio.buffers.capacity()),
        );
        let mut timings = Vec::with_capacity(MEASUREMENTS);
        let mut checksum = 0.0f64;
        for _ in 0..MEASUREMENTS {
            let start = Instant::now();
            let packet = audio.submit(FRAMES).unwrap();
            let elapsed = start.elapsed().as_nanos();
            timings.push(elapsed);

            for &sample in audio.submission_pcm(packet).unwrap() {
                assert!(sample.is_finite() && sample.abs() <= 1., "{profile}");
                checksum += f64::from(sample.abs());
            }
            assert_eq!(
                (
                    (audio.output.as_ptr(), audio.output.capacity()),
                    (audio.wide_output.as_ptr(), audio.wide_output.capacity()),
                    (audio.spans.as_ptr(), audio.spans.capacity()),
                    (audio.voices.as_ptr(), audio.voices.capacity()),
                    (audio.buffers.as_ptr(), audio.buffers.capacity()),
                ),
                storage
            );
            let chain = audio.voice_effects(voice).unwrap().unwrap();
            assert_eq!(
                (chain.effects.as_ptr(), chain.effects.capacity()),
                effects_storage
            );
            for (effect, &expected) in chain.effects.iter().zip(&delay_storage) {
                assert_eq!(
                    (
                        effect.delay.samples.as_ptr(),
                        effect.delay.samples.capacity()
                    ),
                    expected
                );
            }
            audio.acknowledge(packet, FRAMES).unwrap();
        }
        assert_eq!(timings.len(), MEASUREMENTS);
        assert!(checksum.is_finite() && checksum > 0., "{profile}");
        timings.sort_unstable();
        let median = timings[MEASUREMENTS / 2];
        let p95 = timings[(MEASUREMENTS * 95).div_ceil(100) - 1];
        let fixed_state_bytes =
            effects_storage.1 * core::mem::size_of::<super::super::chain::Effect>();
        println!(
            "{{\"profile\":{profile:?},\"sample_rate\":48000,\"voices\":1,\"effects\":{},\"frames\":{FRAMES},\"warmups\":{WARMUPS},\"measurements\":{MEASUREMENTS},\"submit_median_ns\":{median},\"submit_p95_ns\":{p95},\"deadline_ns\":10000000,\"effect_delay_payload_bytes\":{},\"effect_fixed_state_bytes\":{fixed_state_bytes},\"checksum\":{checksum}}}",
            kinds.len(),
            audio.effect_bytes,
        );
    }
}
