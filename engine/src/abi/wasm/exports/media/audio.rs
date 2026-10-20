export_process_calls! {
    audio_create_buffer(sample_rate: u32, channels: u32, bits: u32, length: u32);
    audio_write_buffer(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32, offset: u32, length: u32);
    audio_release_buffer(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
    audio_create_voice(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
    audio_release_voice(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
    audio_play(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32, looping: u32);
    audio_stop(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
    audio_seek(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32, frame: u32);
    audio_submit(frames: u32);
    audio_enqueue(frames: u32);
    audio_acknowledge(epoch_low: u32, epoch_high: u32, seq_low: u32, seq_high: u32, consumed: u32);
    audio_state();
    audio_read_submission(epoch_low: u32, epoch_high: u32, seq_low: u32, seq_high: u32, first: u32, frames: u32);
    audio_voice_state(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
}
