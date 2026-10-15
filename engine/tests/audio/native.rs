mod buffers;
mod playback;
mod submission;

use ring3_engine::audio::{AudioConfig32, AudioContext, PcmFormat32};

fn context() -> AudioContext {
    AudioContext::new(
        7,
        AudioConfig32 {
            sample_rate: 48000,
            max_buffers: 4,
            max_voices: 4,
            max_buffer_bytes: 128,
            render_frames: 8,
        },
    )
    .unwrap()
}

fn pcm(channels: u16, bits_per_sample: u16) -> PcmFormat32 {
    PcmFormat32 {
        sample_rate: 48000,
        channels,
        bits_per_sample,
    }
}
