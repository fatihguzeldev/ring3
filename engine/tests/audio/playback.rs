use super::{context, pcm};
use ring3_engine::audio::{AudioBufferId32, AudioError32, AudioVoiceId32};

#[test]
fn loop_seek_stop_and_nonloop_tail_follow_consumed_frames() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(1, 8), 3).unwrap();
    audio.write_buffer(buffer, 0, &[128, 192, 64]).unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    audio.seek(voice, 2).unwrap();
    audio.play(voice, true).unwrap();
    let block = audio.submit(8).unwrap();
    assert_eq!(
        audio.submission_pcm(block).unwrap(),
        &[
            -0.5, -0.5, 0.0, 0.0, 0.5, 0.5, -0.5, -0.5, 0.0, 0.0, 0.5, 0.5, -0.5, -0.5, 0.0, 0.0
        ]
    );
    let ahead = audio.voice_state(voice).unwrap();
    assert_eq!(
        (ahead.position_frames, ahead.produced_position_frames),
        (2, 1)
    );
    audio.acknowledge(block, 4).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
    audio.acknowledge(block, 8).unwrap();
    audio.stop(voice).unwrap();
    let stopped = audio.voice_state(voice).unwrap();
    assert_eq!(stopped.position_frames, 1);
    assert!(!stopped.playing && !stopped.producing);
    let silence = audio.submit(2).unwrap();
    assert_eq!(audio.submission_pcm(silence).unwrap(), &[0.0; 4]);
    audio.acknowledge(silence, 2).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap(), stopped);

    audio.seek(voice, 0).unwrap();
    audio.play(voice, false).unwrap();
    let first = audio.submit(2).unwrap();
    assert_eq!(audio.submission_pcm(first).unwrap(), &[0.0, 0.0, 0.5, 0.5]);
    audio.acknowledge(first, 2).unwrap();
    let tail = audio.submit(2).unwrap();
    assert_eq!(audio.submission_pcm(tail).unwrap(), &[-0.5, -0.5, 0.0, 0.0]);
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 2);
    audio.acknowledge(tail, 1).unwrap();
    let ended = audio.voice_state(voice).unwrap();
    assert_eq!(ended.position_frames, 0);
    assert!(!ended.playing && !ended.producing);
    audio.acknowledge(tail, 2).unwrap();
    audio.play(voice, false).unwrap();
    let replay = audio.submit(1).unwrap();
    assert_eq!(audio.submission_pcm(replay).unwrap(), &[0.0, 0.0]);
}

#[test]
fn multiple_voices_mix_in_fixed_slot_order_and_clip_only_the_final_output() {
    let mut audio = context();
    let mono = audio.create_buffer(pcm(1, 8), 2).unwrap();
    audio.write_buffer(mono, 0, &[192, 64]).unwrap();
    let stereo = audio.create_buffer(pcm(2, 8), 4).unwrap();
    audio.write_buffer(stereo, 0, &[192, 0, 128, 192]).unwrap();
    let a = audio.create_voice(mono).unwrap();
    let b = audio.create_voice(stereo).unwrap();
    audio.play(a, false).unwrap();
    audio.play(b, false).unwrap();
    let two = audio.submit(2).unwrap();
    assert_eq!(audio.submission_pcm(two).unwrap(), &[1.0, -0.5, -0.5, 0.0]);
    audio.acknowledge(two, 2).unwrap();
    let c = audio.create_voice(mono).unwrap();
    for voice in [a, b, c] {
        audio.play(voice, false).unwrap();
    }
    let three = audio.submit(2).unwrap();
    assert_eq!(
        audio.submission_pcm(three).unwrap(),
        &[1.0, 0.0, -1.0, -0.5]
    );
}

#[test]
fn buffer_and_voice_generations_reject_released_or_forged_handles() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(1, 8), 2).unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    assert_eq!(audio.release_buffer(buffer), Err(AudioError32::BufferInUse));
    audio.release_voice(voice).unwrap();
    assert_eq!(audio.play(voice, false), Err(AudioError32::InvalidVoice));
    audio.release_buffer(buffer).unwrap();
    assert_eq!(
        audio.write_buffer(buffer, 0, &[128]),
        Err(AudioError32::InvalidBuffer)
    );
    let replacement = audio.create_buffer(pcm(1, 8), 2).unwrap();
    assert_eq!(
        (replacement.slot, replacement.generation),
        (buffer.slot, buffer.generation + 1)
    );
    let next_voice = audio.create_voice(replacement).unwrap();
    assert_eq!(
        (next_voice.slot, next_voice.generation),
        (voice.slot, voice.generation + 1)
    );
    for invalid in [
        buffer,
        AudioBufferId32 {
            epoch: 8,
            ..replacement
        },
        AudioBufferId32 {
            generation: 0,
            ..replacement
        },
        AudioBufferId32 {
            slot: u32::MAX,
            ..replacement
        },
    ] {
        assert_eq!(
            audio.release_buffer(invalid),
            Err(AudioError32::InvalidBuffer)
        );
    }
    for invalid in [
        voice,
        AudioVoiceId32 {
            epoch: 8,
            ..next_voice
        },
        AudioVoiceId32 {
            generation: 0,
            ..next_voice
        },
        AudioVoiceId32 {
            slot: u32::MAX,
            ..next_voice
        },
    ] {
        assert_eq!(audio.voice_state(invalid), Err(AudioError32::InvalidVoice));
    }
    let before = audio.voice_state(next_voice).unwrap();
    assert_eq!(audio.seek(next_voice, 2), Err(AudioError32::InvalidRequest));
    assert_eq!(audio.voice_state(next_voice).unwrap(), before);
}
