use super::*;

#[test]
fn pending_packet_stays_immutable_while_wrapped_stream_data_updates_next_packet() {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(48_000), SampleEncoding32::Integer, 4)
        .unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[128; 4], &[])
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, true).unwrap();
    let packet = audio.submit(4).unwrap();
    let pointer = audio.submission_pcm(packet).unwrap().as_ptr();
    audio.write_stream_buffer(buffer, 3, &[192], &[64]).unwrap();
    assert_eq!(audio.submission_pcm(packet).unwrap(), &[0.0; 8]);
    assert_eq!(audio.submission_pcm(packet).unwrap().as_ptr(), pointer);
    audio.acknowledge(packet, 4).unwrap();
    let next = audio.submit(4).unwrap();
    assert_eq!(
        audio.submission_pcm(next).unwrap(),
        &[-0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]
    );
}

#[test]
fn old_partial_ack_cannot_overwrite_seek_stop_or_reused_voice_identity() {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(48_000), SampleEncoding32::Integer, 4)
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, true).unwrap();
    let packet = audio.submit(4).unwrap();
    audio.acknowledge(packet, 1).unwrap();
    audio.seek_stream(voice, 3).unwrap();
    assert_eq!(audio.acknowledgement_progress(packet, 2, voice), Ok(None));
    audio.acknowledge(packet, 2).unwrap();
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 3);
    audio.stop_stream(voice).unwrap();
    audio.release_stream_voice(voice).unwrap();
    let reused = audio.create_stream_voice(buffer).unwrap();
    assert_eq!(reused.slot, voice.slot);
    assert_ne!(reused.generation, voice.generation);
    audio.play_stream(reused, true).unwrap();
    assert_eq!(audio.acknowledgement_progress(packet, 4, reused), Ok(None));
    audio.acknowledge(packet, 4).unwrap();
    assert_eq!(audio.voice_state(reused).unwrap().position_frames, 0);
    assert!(audio.voice_state(reused).unwrap().playing);
}

#[test]
fn consumed_progress_distinguishes_multiple_wraps_from_unchanged_cursor() {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(48_000), SampleEncoding32::Integer, 2)
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, true).unwrap();
    let packet = audio.submit(6).unwrap();
    let mut progress = None;
    audio
        .visit_acknowledgement(packet, 6, |value| progress = Some(value))
        .unwrap();
    let progress = progress.unwrap();
    assert_eq!(
        audio.acknowledgement_progress(packet, 6, voice),
        Ok(Some(progress))
    );
    assert_eq!(
        audio.can_acknowledge(packet, 0),
        Err(super::AudioError32::InvalidRequest)
    );
    assert_eq!(
        (
            progress.first_phase,
            progress.last_phase,
            progress.wraps,
            progress.stopped
        ),
        (0, 0, 3, false)
    );
    assert_eq!(audio.voice_state(voice).unwrap().position_frames, 0);
}

#[test]
fn repeated_play_preserves_pending_end_and_only_new_looping_flags_resume_production() {
    for looping in [false, true] {
        let mut audio = context();
        let buffer = audio
            .create_stream_buffer(mono(48_000), SampleEncoding32::Integer, 2)
            .unwrap();
        audio
            .write_stream_buffer(buffer, 0, &[192; 2], &[])
            .unwrap();
        let voice = audio.create_stream_voice(buffer).unwrap();
        audio.play_stream(voice, false).unwrap();
        let packet = audio.submit(4).unwrap();
        assert!(!audio.voice_state(voice).unwrap().producing);
        audio.play_stream(voice, looping).unwrap();
        assert_eq!(audio.voice_state(voice).unwrap().producing, looping);
        let mut stopped = None;
        audio
            .visit_acknowledgement(packet, 4, |progress| stopped = Some(progress.stopped))
            .unwrap();
        assert_eq!(stopped, Some(!looping));
        assert_eq!(
            audio.submission_pcm(packet).unwrap(),
            &[0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]
        );
        audio.acknowledge(packet, 4).unwrap();
        assert_eq!(audio.voice_state(voice).unwrap().playing, looping);
        let next = audio.submit(2).unwrap();
        assert_eq!(
            audio.submission_pcm(next).unwrap(),
            if looping { &[0.5; 4] } else { &[0.0; 4] }
        );
    }
}
