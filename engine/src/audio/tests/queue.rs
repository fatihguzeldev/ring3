use super::*;

fn stream() -> (AudioContext, AudioBufferId32, AudioVoiceId32) {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(48_000), SampleEncoding32::Integer, 4)
        .unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[128, 160, 192, 224], &[])
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, true).unwrap();
    (audio, buffer, voice)
}

#[test]
fn queue_keeps_each_packet_and_only_the_head_can_advance_consumption() {
    let (mut audio, _, voice) = stream();
    let first = audio.enqueue(3).unwrap();
    let second = audio.enqueue(2).unwrap();
    let pointer = audio.submission_pcm(first).unwrap().as_ptr();
    assert_eq!(
        audio.submission_pcm(first).unwrap(),
        &[0., 0., 0.25, 0.25, 0.5, 0.5]
    );
    assert_eq!(audio.submission_pcm(second).unwrap(), &[0.75, 0.75, 0., 0.]);
    assert_eq!((first.first_frame, second.first_frame), (0, 3));
    assert_eq!(
        (
            audio.state().submitted_frames,
            audio.state().consumed_frames
        ),
        (5, 0)
    );
    assert_eq!(audio.state().pending, Some(first));
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
    assert_eq!(
        audio.voice_state(voice).unwrap().produced_position_frames,
        1
    );
    let before = (audio.state(), audio.voice_state(voice).unwrap());
    assert_eq!(
        audio.acknowledge(second, 1),
        Err(AudioError32::InvalidSubmission)
    );
    assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    audio.acknowledge(first, 2).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 2);
    assert_eq!(audio.state().consumed_frames, 2);
    assert_eq!(audio.submission_pcm(first).unwrap().as_ptr(), pointer);
    assert_eq!(audio.submission_pcm(second).unwrap(), &[0.75, 0.75, 0., 0.]);
    audio.acknowledge(first, 3).unwrap();
    assert_eq!(audio.state().pending, Some(second));
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 3);
    audio.acknowledge(second, 1).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
    audio.acknowledge(second, 2).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 1);
    assert_eq!(audio.state().consumed_frames, 5);
    assert_eq!(audio.state().pending, None);
    assert_eq!(
        audio.acknowledge(first, 3),
        Err(AudioError32::InvalidSubmission)
    );
}

#[test]
fn full_queue_is_bounded_and_failed_enqueue_does_not_render_or_reuse_pinned_pcm() {
    let (mut audio, _, voice) = stream();
    let packets: Vec<_> = (0..8).map(|_| audio.enqueue(1).unwrap()).collect();
    let pcm: Vec<_> = packets
        .iter()
        .map(|id| audio.submission_pcm(*id).unwrap().to_vec())
        .collect();
    let before = (audio.state(), audio.voice_state(voice).unwrap());
    assert_eq!(audio.enqueue(1), Err(AudioError32::Busy));
    assert_eq!(audio.submit(1), Err(AudioError32::Busy));
    assert_eq!((audio.state(), audio.voice_state(voice).unwrap()), before);
    audio.acknowledge(packets[0], 1).unwrap();
    let next = audio.enqueue(1).unwrap();
    assert_eq!((next.sequence, next.first_frame), (9, 8));
    assert_eq!(audio.enqueue(1), Err(AudioError32::Busy));
    for (id, expected) in packets.iter().zip(&pcm).skip(1) {
        assert_eq!(audio.submission_pcm(*id).unwrap(), expected);
        audio.acknowledge(*id, 1).unwrap();
    }
    assert_eq!(audio.state().pending, Some(next));
    audio.acknowledge(next, 1).unwrap();
    assert_eq!(audio.state().consumed_frames, 9);
}

#[test]
fn full_queue_stream_mutations_do_not_rewrite_pcm_or_let_old_revisions_rewind_the_voice() {
    let (mut audio, buffer, voice) = stream();
    let packets: Vec<_> = (0..8).map(|_| audio.enqueue(1).unwrap()).collect();
    let pcm: Vec<_> = packets
        .iter()
        .map(|id| audio.submission_pcm(*id).unwrap().to_vec())
        .collect();
    audio.write_stream_buffer(buffer, 3, &[64], &[192]).unwrap();
    audio.seek_stream(voice, 3).unwrap();
    audio.stop_stream(voice).unwrap();
    audio.play_stream(voice, true).unwrap();
    for (id, expected) in packets.iter().zip(&pcm) {
        assert_eq!(audio.submission_pcm(*id).unwrap(), expected);
        assert_eq!(
            audio.acknowledgement_progress(*id, 1, voice),
            if *id == packets[0] {
                Ok(None)
            } else {
                Err(AudioError32::InvalidSubmission)
            }
        );
    }
    audio.acknowledge(packets[0], 1).unwrap();
    let next = audio.enqueue(1).unwrap();
    assert_eq!(audio.submission_pcm(next).unwrap(), &[-0.5, -0.5]);
    for id in packets.into_iter().skip(1) {
        audio.acknowledge(id, 1).unwrap();
        let state = audio.voice_state(voice).unwrap();
        assert_eq!(state.position_frames, 3);
        assert!(state.playing);
    }
    audio.acknowledge(next, 1).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
}

#[test]
fn queued_old_voice_generations_cannot_update_a_reused_slot() {
    let (mut audio, buffer, old) = stream();
    let first = audio.enqueue(2).unwrap();
    let second = audio.enqueue(2).unwrap();
    audio.release_stream_voice(old).unwrap();
    let replacement = audio.create_stream_voice(buffer).unwrap();
    assert_eq!(replacement.slot, old.slot);
    assert_ne!(replacement.generation, old.generation);
    audio.seek_stream(replacement, 3).unwrap();
    audio.play_stream(replacement, true).unwrap();
    for id in [first, second] {
        audio.acknowledge(id, 2).unwrap();
        assert_eq!(audio.voice_state(replacement).unwrap().position_frames, 3);
    }
}

#[test]
fn reset_cancels_all_queued_identities_and_termination_leaves_exact_pcm_drainable() {
    let (mut audio, _, _) = stream();
    let first = audio.enqueue(2).unwrap();
    let second = audio.enqueue(3).unwrap();
    let pcm: Vec<_> = [first, second]
        .map(|id| audio.submission_pcm(id).unwrap().to_vec())
        .into();
    audio.terminate_process();
    for (id, expected) in [first, second].into_iter().zip(pcm) {
        assert_eq!(audio.submission_pcm(id).unwrap(), expected);
        audio.acknowledge(id, id.frames).unwrap();
    }
    assert_eq!(audio.state().consumed_frames, 5);
    let cancelled = [audio.enqueue(1).unwrap(), audio.enqueue(1).unwrap()];
    audio.reset(4).unwrap();
    assert_eq!(
        (
            audio.state().submitted_frames,
            audio.state().consumed_frames,
            audio.state().pending
        ),
        (0, 0, None)
    );
    for id in cancelled {
        assert_eq!(
            audio.acknowledge(id, 1),
            Err(AudioError32::InvalidSubmission)
        );
        assert_eq!(
            audio.submission_pcm(id),
            Err(AudioError32::InvalidSubmission)
        );
    }
    let next = audio.enqueue(1).unwrap();
    assert_eq!((next.epoch, next.sequence, next.first_frame), (4, 1, 0));
}
