use super::{super::objects::ObjectKind32, support::*};
use crate::{
    audio::dsound::{BufferDescription, HResult},
    memory::GuestAddress,
};
#[test]
fn wrapped_partial_unlock_updates_future_pcm_and_exhausted_unmap_is_neutral() {
    let (mut runtime, mut memory, mut audio, device) = context();
    let pointer = buffer(
        &mut runtime,
        &mut memory,
        &mut audio,
        device,
        BufferDescription::NOTIFY,
    );
    let index = runtime.slot(pointer).unwrap();
    let ObjectKind32::Buffer(buffer) = &runtime.records[index].as_ref().unwrap().kind else {
        unreachable!()
    };
    let storage = buffer.native.storage;
    let voice = buffer.native.voice;
    audio.play_stream(voice, true).unwrap();
    let pending = audio.submit(4).unwrap();
    assert_eq!(audio.submission_pcm(pending).unwrap(), &[0.0; 8]);
    runtime
        .lock_buffer(
            &mut memory,
            &audio,
            pointer,
            &[
                pointer.0,
                3,
                3,
                DATA + 20,
                DATA + 24,
                DATA + 28,
                DATA + 32,
                0,
            ],
        )
        .unwrap();
    let first = read(&memory, DATA + 20);
    let second = read(&memory, DATA + 28);
    assert_eq!((read(&memory, DATA + 24), read(&memory, DATA + 32)), (1, 2));
    memory.write(GuestAddress(first), &[192]).unwrap();
    memory.write(GuestAddress(second), &[64, 255]).unwrap();
    let version = memory.transaction_version();
    memory.exhaust_versions_for_test();
    assert_eq!(
        runtime.unlock_buffer(
            &mut memory,
            &mut audio,
            pointer,
            &[pointer.0, first, 1, second, 1]
        ),
        Err(HResult::INVALID_PARAM)
    );
    assert_eq!(audio.stream_bytes(storage).unwrap(), &[128; 4]);
    assert_eq!(read(&memory, first) & 255, 192);
    assert_eq!(runtime.locks.entries.len(), 1);
    memory.restore_version_for_test(version);
    runtime
        .unlock_buffer(
            &mut memory,
            &mut audio,
            pointer,
            &[pointer.0, first, 1, second, 1],
        )
        .unwrap();
    assert_eq!(audio.stream_bytes(storage).unwrap(), &[64, 128, 128, 192]);
    assert_eq!(audio.submission_pcm(pending).unwrap(), &[0.0; 8]);
    assert!(memory.read(GuestAddress(first), &mut [0]).is_err());
    audio.acknowledge(pending, 4).unwrap();
    let next = audio.submit(4).unwrap();
    assert_eq!(
        audio.submission_pcm(next).unwrap(),
        &[-0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]
    );
}

#[test]
fn capture_unlock_preserves_new_host_pcm_while_publishing_changed_guest_bytes() {
    let (mut runtime, mut memory, mut audio, _) = context();
    let pointer = capture_buffer(&mut runtime, &mut memory, &mut audio);
    let identity = runtime.com.lookup(pointer).unwrap().id.get();
    let slot = runtime.slot(pointer).unwrap();
    let feed = |runtime: &mut super::super::DSoundRuntime32, values: &[u8]| {
        let ObjectKind32::CaptureBuffer(buffer) = &mut runtime.records[slot].as_mut().unwrap().kind
        else {
            unreachable!()
        };
        let request = buffer.native.request(7, identity).unwrap();
        let prepared = buffer
            .native
            .prepare_write(request, 7, identity, values)
            .unwrap();
        buffer.native.commit_write(prepared, values);
    };
    let ObjectKind32::CaptureBuffer(buffer) = &mut runtime.records[slot].as_mut().unwrap().kind
    else {
        unreachable!()
    };
    buffer.native.start(1).unwrap();
    feed(&mut runtime, &[1, 2, 3, 4]);
    runtime
        .lock_buffer(
            &mut memory,
            &audio,
            pointer,
            &[
                pointer.0,
                3,
                3,
                DATA + 20,
                DATA + 24,
                DATA + 28,
                DATA + 32,
                0,
            ],
        )
        .unwrap();
    let first = read(&memory, DATA + 20);
    let second = read(&memory, DATA + 28);
    feed(&mut runtime, &[11, 12, 13, 14]);
    memory.write(GuestAddress(second), &[99]).unwrap();
    runtime
        .unlock_buffer(
            &mut memory,
            &mut audio,
            pointer,
            &[pointer.0, first, 1, second, 2],
        )
        .unwrap();
    let ObjectKind32::CaptureBuffer(buffer) = &runtime.records[slot].as_ref().unwrap().kind else {
        unreachable!()
    };
    assert_eq!(buffer.native.bytes(), &[99, 12, 13, 14]);
    assert_eq!(buffer.native.position(), 0);
    assert!(runtime.locks.entries.is_empty());
}
