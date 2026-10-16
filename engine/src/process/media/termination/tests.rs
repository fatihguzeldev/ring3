use crate::{
    audio::{AudioError32, PcmFormat32},
    process::{EngineInstance, HostError, MediaConfig},
};
#[test]
fn postexit_audio_output_drain_keeps_exact_receipt_and_refuses_new_media_work() {
    let mut engine = EngineInstance::new(16, 7).unwrap();
    engine.configure_cpu_clock(1000000, 0).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    let buffer = engine
        .audio_create_buffer(
            PcmFormat32 {
                sample_rate: 48000,
                channels: 1,
                bits_per_sample: 8,
            },
            4,
        )
        .unwrap();
    engine
        .audio_write_buffer(buffer, 0, &[0, 64, 128, 255])
        .unwrap();
    let voice = engine.audio_create_voice(buffer).unwrap();
    engine.audio_play(voice, true).unwrap();
    let submission = engine.audio_submit(4).unwrap();
    let pcm = engine
        .audio()
        .unwrap()
        .submission_pcm(submission)
        .unwrap()
        .to_vec();
    let prepared = engine.prepare_media_process_exit().unwrap();
    assert!(prepared.bindings().is_empty());
    engine.commit_media_process_exit(prepared);
    engine.exit_code = Some(0);
    assert_eq!(
        engine.audio().unwrap().submission_pcm(submission).unwrap(),
        pcm
    );
    assert_eq!(engine.audio_submit(1), Err(HostError::ProcessExited));
    assert!(matches!(engine.input(), Err(HostError::ProcessExited)));
    engine.audio_acknowledge(submission, 4).unwrap();
    assert_eq!(engine.audio().unwrap().state().pending, None);
    assert_eq!(
        engine.audio_acknowledge(submission, 4),
        Err(HostError::Media(super::super::MediaError::Audio(
            AudioError32::InvalidSubmission
        )))
    );
    engine.close();
    assert!(matches!(engine.audio(), Err(HostError::Closed)));
}
