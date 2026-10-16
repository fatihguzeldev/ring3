use super::*;
#[test]
fn process_termination_retires_resources_without_changing_submitted_pcm_or_ack_identity() {
    let mut audio = context();
    let buffer = audio
        .create_stream_buffer(mono(48000), SampleEncoding32::Integer, 4)
        .unwrap();
    audio
        .write_stream_buffer(buffer, 0, &[0, 64, 128, 255], &[])
        .unwrap();
    let voice = audio.create_stream_voice(buffer).unwrap();
    audio.play_stream(voice, true).unwrap();
    let submission = audio.submit(4).unwrap();
    let pcm = audio.submission_pcm(submission).unwrap().to_vec();
    audio.terminate_process();
    assert_eq!(audio.submission_pcm(submission).unwrap(), pcm);
    assert_eq!(audio.voice_state(voice), Err(AudioError32::InvalidVoice));
    assert!(audio.stream_bytes(buffer).is_err());
    audio.acknowledge(submission, 2).unwrap();
    assert_eq!(audio.submission_pcm(submission).unwrap(), pcm);
    audio.acknowledge(submission, 4).unwrap();
    assert_eq!(audio.state().pending, None);
    assert_eq!(
        audio.acknowledge(submission, 4),
        Err(AudioError32::InvalidSubmission)
    );
}
