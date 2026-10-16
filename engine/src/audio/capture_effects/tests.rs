use super::*;
use crate::audio::{PcmFormat32, SampleEncoding32, dsound::WaveFormat};

fn format() -> WaveFormat {
    WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 32,
        },
        encoding: SampleEncoding32::Float,
        extensible: false,
        channel_mask: 0,
    }
}
#[test]
fn adaptive_filter_removes_delayed_echo_and_reset_restores_actual_uninitialized_state() {
    let format = format();
    let mut filter = CaptureProcessor::new(format, &[CaptureEffectKind::Aec], 1 << 20).unwrap();
    let mut reference = Vec::new();
    let mut random = 1u32;
    for _ in 0..16000 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        reference.push(random as i32 as f32 / i32::MAX as f32 * 0.25)
    }
    let mut capture = Vec::new();
    for index in 0..reference.len() {
        capture.extend_from_slice(
            &(if index < 37 {
                0.
            } else {
                reference[index - 37] * 0.6
            })
            .to_le_bytes(),
        )
    }
    filter.process(&mut capture, AecParameters::default(), false, |index| {
        Some([reference[index]; 2])
    });
    let residual = capture[48000..]
        .chunks_exact(4)
        .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()).powi(2))
        .sum::<f32>();
    let source = reference[12000..]
        .iter()
        .map(|sample| sample.powi(2))
        .sum::<f32>();
    assert!(
        residual < source * 0.05,
        "residual/source = {}",
        residual / source
    );
    assert_eq!(filter.mode(), 2);
    assert_eq!(filter.status() & 8, 8);
    filter.reset();
    assert_eq!(filter.status(), 0);
    assert_eq!(filter.mode(), 0);
}
#[test]
fn missing_reference_and_disabled_aec_preserve_capture_bytes_without_fake_processing() {
    let format = format();
    let mut filter = CaptureProcessor::new(
        format,
        &[CaptureEffectKind::Aec, CaptureEffectKind::NoiseSuppress],
        1 << 20,
    )
    .unwrap();
    let original: Vec<_> = (0..512)
        .flat_map(|index| (index as f32 / 1024.).to_le_bytes())
        .collect();
    let mut capture = original.clone();
    filter.process(&mut capture, AecParameters::default(), true, |_| None);
    assert_eq!(capture, original);
    assert_eq!(filter.mode(), 0);
    filter.process(
        &mut capture,
        AecParameters {
            enabled: false,
            noise_fill: true,
        },
        true,
        |_| Some([0.5; 2]),
    );
    assert_eq!(capture, original);
    assert!(matches!(
        CaptureProcessor::new(format, &[CaptureEffectKind::NoiseSuppress], 1 << 20),
        Err(crate::audio::dsound::HResult::INVALID_PARAM)
    ));
    assert!(matches!(
        CaptureProcessor::new(format, &[CaptureEffectKind::Aec], 1),
        Err(crate::audio::dsound::HResult::OUT_OF_MEMORY)
    ));
}
#[test]
fn noise_suppression_uses_its_measured_spectrum_and_preserves_disabled_capture() {
    let format = format();
    let mut filter = CaptureProcessor::new(
        format,
        &[CaptureEffectKind::Aec, CaptureEffectKind::NoiseSuppress],
        1 << 20,
    )
    .unwrap();
    let mut capture: Vec<_> = (0..8192).flat_map(|_| 0.025f32.to_le_bytes()).collect();
    filter.process(&mut capture, AecParameters::default(), true, |_| {
        Some([0.; 2])
    });
    let power = capture[16000..]
        .chunks_exact(4)
        .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()).powi(2))
        .sum::<f32>();
    assert!(power < 0.025 * 0.025 * 4096. * 0.05);
    filter.reset_noise();
    let mut capture: Vec<_> = (0..512).flat_map(|_| 0.025f32.to_le_bytes()).collect();
    filter.process(&mut capture, AecParameters::default(), false, |_| {
        Some([0.; 2])
    });
    assert!(
        capture[1024..]
            .chunks_exact(4)
            .all(|sample| f32::from_le_bytes(sample.try_into().unwrap()) == 0.025)
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "tests/performance.rs"]
mod performance;

#[test]
fn processor_owns_a_validated_format_before_allocating_or_accepting_pcm() {
    let mut invalid = format();
    invalid.pcm.channels = 3;
    assert!(matches!(
        CaptureProcessor::new(invalid, &[CaptureEffectKind::Aec], 1 << 20),
        Err(crate::audio::dsound::HResult::BAD_FORMAT)
    ));
    invalid = format();
    invalid.pcm.bits_per_sample = 0;
    assert!(matches!(
        CaptureProcessor::new(invalid, &[CaptureEffectKind::Aec], 1 << 20),
        Err(crate::audio::dsound::HResult::BAD_FORMAT)
    ));
}
