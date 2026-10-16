use super::super::*;
use crate::audio::{PcmFormat32, SampleEncoding32, dsound::WaveFormat};
use std::time::Instant;

#[test]
#[ignore = "explicit release-mode capture DSP measurement"]
fn realtime_48khz_128ms_filter_reports_preallocated_ten_millisecond_chunks() {
    for channels in [1, 2] {
        let format = WaveFormat {
            pcm: PcmFormat32 {
                sample_rate: 48000,
                channels,
                bits_per_sample: 16,
            },
            encoding: SampleEncoding32::Integer,
            extensible: false,
            channel_mask: 0,
        };
        let mut processor = CaptureProcessor::new(
            format,
            &[CaptureEffectKind::Aec, CaptureEffectKind::NoiseSuppress],
            4 * 1024 * 1024,
        )
        .unwrap();
        let charged = processor.bytes();
        let mut bytes = vec![0; 480 * channels as usize * 2];
        let reference = (0..(480 * 400 + 64))
            .map(|index| {
                (((index as u32)
                    .wrapping_mul(747796405)
                    .wrapping_add(2891336453)
                    >> 8) as i32
                    % 16000
                    - 8000) as f32
                    / 32768.
            })
            .collect::<Vec<_>>();
        let mut elapsed = Vec::with_capacity(300);
        for chunk in 0..400 {
            let first = chunk * 480;
            for (index, frame) in bytes.chunks_exact_mut(channels as usize * 2).enumerate() {
                let sample = (reference[first + index + 27] * 19660.).round() as i16;
                for sample_bytes in frame.chunks_exact_mut(2) {
                    sample_bytes.copy_from_slice(&sample.to_le_bytes())
                }
            }
            let started = Instant::now();
            processor.process(&mut bytes, AecParameters::default(), true, |index| {
                Some([reference[first + index + 64]; 2])
            });
            let duration = started.elapsed().as_nanos();
            if chunk >= 100 {
                elapsed.push(duration)
            }
            assert_eq!(processor.bytes(), charged);
        }
        elapsed.sort_unstable();
        let median = elapsed[elapsed.len() / 2];
        let p95 = elapsed[elapsed.len() * 95 / 100];
        println!(
            "{{\"sample_rate\":48000,\"channels\":{channels},\"history_ms\":128,\"chunk_frames\":480,\"resident_bytes\":{charged},\"median_ns\":{median},\"p95_ns\":{p95},\"deadline_ns\":10000000}}"
        );
        assert_eq!(processor.mode(), 2);
    }
}
