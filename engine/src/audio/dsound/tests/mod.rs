mod buffers;
mod capture;
mod spatial;

use super::*;
use crate::audio::{AudioConfig32, AudioContext, PcmFormat32, SampleEncoding32};
fn format() -> WaveFormat {
    WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 48_000,
            channels: 1,
            bits_per_sample: 8,
        },
        encoding: SampleEncoding32::Integer,
        extensible: false,
        channel_mask: 0,
    }
}
fn audio() -> AudioContext {
    AudioContext::new(
        1,
        AudioConfig32 {
            render_frames: 8,
            ..AudioConfig32::default()
        },
    )
    .unwrap()
}
