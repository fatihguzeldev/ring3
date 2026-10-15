use super::{context, pcm};
use ring3_engine::audio::{AudioError32, AudioSubmission32};

#[test]
fn exact_partial_ack_advances_consumption_without_rewriting_pinned_pcm() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(1, 8), 4).unwrap();
    audio
        .write_buffer(buffer, 0, &[128, 160, 192, 224])
        .unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    audio.play(voice, false).unwrap();
    let block = audio.submit(4).unwrap();
    assert_eq!(
        block,
        AudioSubmission32 {
            epoch: 7,
            sequence: 1,
            first_frame: 0,
            frames: 4
        }
    );
    let pointer = audio.submission_pcm(block).unwrap().as_ptr();
    assert_eq!(
        audio.submission_pcm(block).unwrap(),
        &[0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75]
    );
    assert_eq!(
        (
            audio.state().submitted_frames,
            audio.state().consumed_frames
        ),
        (4, 0)
    );
    assert!(audio.voice_state(voice).unwrap().playing);
    assert!(!audio.voice_state(voice).unwrap().producing);
    for invalid in [
        AudioSubmission32 { epoch: 8, ..block },
        AudioSubmission32 {
            sequence: 2,
            ..block
        },
        AudioSubmission32 {
            first_frame: 1,
            ..block
        },
        AudioSubmission32 { frames: 3, ..block },
    ] {
        let before = (audio.state(), audio.voice_state(voice).unwrap());
        assert_eq!(
            audio.acknowledge(invalid, 1),
            Err(AudioError32::InvalidSubmission)
        );
        assert_eq!(
            audio.submission_pcm(invalid),
            Err(AudioError32::InvalidSubmission)
        );
        assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    }
    for invalid in [0, 5] {
        assert_eq!(
            audio.acknowledge(block, invalid),
            Err(AudioError32::InvalidRequest)
        );
    }
    audio.acknowledge(block, 2).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 2);
    assert_eq!(audio.state().consumed_frames, 2);
    assert_eq!(audio.submission_pcm(block).unwrap().as_ptr(), pointer);
    assert_eq!(
        audio.submission_pcm(block).unwrap(),
        &[0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75]
    );
    let before = (audio.state(), audio.voice_state(voice).unwrap());
    for invalid in [1, 2, 5] {
        assert_eq!(
            audio.acknowledge(block, invalid),
            Err(AudioError32::InvalidRequest)
        );
    }
    assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    audio.acknowledge(block, 4).unwrap();
    assert_eq!(audio.state().consumed_frames, 4);
    assert_eq!(audio.state().pending, None);
    assert_eq!(
        audio.submission_pcm(block),
        Err(AudioError32::InvalidSubmission)
    );
    assert_eq!(
        audio.acknowledge(block, 4),
        Err(AudioError32::InvalidSubmission)
    );
    let next = audio.submit(2).unwrap();
    assert_eq!((next.sequence, next.first_frame), (2, 4));
    assert_eq!(audio.submission_pcm(next).unwrap().as_ptr(), pointer);
}

#[test]
fn pending_output_blocks_mutations_and_invalid_requests_leave_state_and_pcm_unchanged() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(1, 8), 2).unwrap();
    audio.write_buffer(buffer, 0, &[64, 192]).unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    audio.play(voice, true).unwrap();
    let block = audio.submit(4).unwrap();
    let before = (audio.state(), audio.voice_state(voice).unwrap());
    assert_eq!(audio.submit(1), Err(AudioError32::Busy));
    assert_eq!(audio.stop(voice), Err(AudioError32::Busy));
    assert_eq!(audio.play(voice, false), Err(AudioError32::Busy));
    assert_eq!(audio.seek(voice, 1), Err(AudioError32::Busy));
    assert_eq!(
        audio.write_buffer(buffer, 0, &[128]),
        Err(AudioError32::Busy)
    );
    assert_eq!(audio.release_voice(voice), Err(AudioError32::Busy));
    assert_eq!(audio.release_buffer(buffer), Err(AudioError32::Busy));
    assert_eq!(audio.create_voice(buffer), Err(AudioError32::Busy));
    assert_eq!(audio.create_buffer(pcm(1, 8), 1), Err(AudioError32::Busy));
    assert_eq!(audio.reset(7), Err(AudioError32::InvalidRequest));
    assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    assert_eq!(
        audio.submission_pcm(block).unwrap(),
        &[-0.5, -0.5, 0.5, 0.5, -0.5, -0.5, 0.5, 0.5]
    );
    audio.acknowledge(block, 4).unwrap();
    let ready = (audio.state(), audio.voice_state(voice).unwrap());
    assert_eq!(audio.submit(0), Err(AudioError32::InvalidRequest));
    assert_eq!(audio.submit(9), Err(AudioError32::InvalidRequest));
    assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), ready);
}

#[test]
fn reset_cancels_pending_output_and_invalidates_old_handles_and_acknowledgements() {
    let mut audio = context();
    let buffer = audio.create_buffer(pcm(1, 8), 2).unwrap();
    let voice = audio.create_voice(buffer).unwrap();
    audio.play(voice, true).unwrap();
    let old = audio.submit(2).unwrap();
    audio.acknowledge(old, 1).unwrap();
    audio.reset(8).unwrap();
    assert_eq!(audio.state().epoch, 8);
    assert_eq!(
        (
            audio.state().submitted_frames,
            audio.state().consumed_frames
        ),
        (0, 0)
    );
    assert_eq!(audio.state().pending, None);
    assert_eq!(
        audio.acknowledge(old, 2),
        Err(AudioError32::InvalidSubmission)
    );
    assert_eq!(
        audio.submission_pcm(old),
        Err(AudioError32::InvalidSubmission)
    );
    assert_eq!(
        audio.release_buffer(buffer),
        Err(AudioError32::InvalidBuffer)
    );
    assert_eq!(audio.play(voice, true), Err(AudioError32::InvalidVoice));
    let replacement = audio.create_buffer(pcm(1, 8), 2).unwrap();
    let next_voice = audio.create_voice(replacement).unwrap();
    assert_eq!(replacement.epoch, 8);
    assert_ne!(replacement.generation, buffer.generation);
    assert_ne!(next_voice.generation, voice.generation);
    let next = audio.submit(2).unwrap();
    assert_eq!((next.epoch, next.sequence), (8, 1));
    assert_eq!(
        audio.acknowledge(old, 2),
        Err(AudioError32::InvalidSubmission)
    );
    assert_eq!(audio.submission_pcm(next).unwrap(), &[0.0; 4]);
}
