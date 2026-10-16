mod resampling;
mod streaming;
mod termination;

use super::*;

fn context() -> AudioContext {
    AudioContext::new(
        3,
        AudioConfig32 {
            sample_rate: 48_000,
            render_frames: 16,
            max_buffers: 4,
            max_voices: 4,
            max_buffer_bytes: 256,
        },
    )
    .unwrap()
}

fn mono(rate: u32) -> PcmFormat32 {
    PcmFormat32 {
        sample_rate: rate,
        channels: 1,
        bits_per_sample: 8,
    }
}
