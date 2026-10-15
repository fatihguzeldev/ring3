use super::{
    super::{memory::write_words, objects::ObjectKind32},
    support::*,
};
use crate::{
    audio::dsound::BufferDescription,
    windows::synchronization::{INFINITE, WaitMode32, WaitOutcome32},
};
#[test]
fn multiple_consumed_wraps_release_multiple_waiters_without_expanding_the_notification_list() {
    let (mut runtime, mut memory, mut audio, device) = context();
    let pointer = buffer(
        &mut runtime,
        &mut memory,
        &mut audio,
        device,
        BufferDescription::NOTIFY,
    );
    let mut sync = sync();
    let (event, _) = sync.create_event(false, false, false, None).unwrap();
    write_words(&mut memory, DATA + 384, &[1, event]).unwrap();
    runtime
        .set_notifications(&memory, &audio, &mut sync, pointer, 1, DATA + 384)
        .unwrap();
    let wait = |sync: &mut crate::windows::synchronization::SynchronizationContext32| {
        let WaitOutcome32::Pending(token) = sync
            .wait(&[event], WaitMode32::Any, INFINITE, false, None)
            .unwrap()
        else {
            panic!("expected pending waiter")
        };
        token
    };
    let first = wait(&mut sync);
    let second = wait(&mut sync);
    let ObjectKind32::Buffer(buffer) = &runtime.object(pointer).unwrap().kind else {
        unreachable!()
    };
    audio.play_stream(buffer.native.voice, true).unwrap();
    let packet = audio.submit(12).unwrap();
    let signals = runtime.prepare_audio_signals(&audio, packet, 12).unwrap();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].1, 3);
    let prepared = sync.prepare_signal_occurrences(signals).unwrap();
    audio.acknowledge(packet, 12).unwrap();
    sync.commit_signals(prepared);
    assert_eq!(sync.wait_status(first).unwrap().unwrap().value, 0);
    assert_eq!(sync.wait_status(second).unwrap().unwrap().value, 0);
    assert_eq!(sync.event_is_signaled(event), Ok(true));
}
