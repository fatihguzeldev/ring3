use super::support::{DATA, buffer, context, read};
use crate::{
    audio::dsound::{BufferDescription, FactoryGate, HResult},
    memory::GuestAddress,
    process::windows::dsound::{memory::write_words, objects::ObjectKind32},
};

#[test]
fn write_primary_locks_real_pcm_excludes_secondary_and_stop_rewinds() {
    let (mut runtime, mut memory, mut audio, device) = context();
    let secondary = buffer(&mut runtime, &mut memory, &mut audio, device, 0);
    runtime
        .buffer_call(
            &mut memory,
            &mut audio,
            None,
            secondary,
            12,
            &[secondary.0, 0, 0, 1],
        )
        .unwrap();
    write_words(
        &mut memory,
        DATA + 128,
        &[36, BufferDescription::PRIMARY, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 4, 0)
        .unwrap();
    let primary = GuestAddress(read(&memory, DATA + 4));
    assert_eq!(
        runtime.lock_buffer(
            &mut memory,
            &audio,
            primary,
            &[primary.0, 0, 4, DATA + 8, DATA + 12, 0, 0, 0]
        ),
        Err(HResult::PRIORITY_LEVEL)
    );
    runtime
        .set_cooperative_level(&mut audio, None, device, 1, 4)
        .unwrap();
    assert_eq!(
        runtime.buffer_call(
            &mut memory,
            &mut audio,
            None,
            secondary,
            12,
            &[secondary.0, 0, 0, 1]
        ),
        Err(HResult::PRIORITY_LEVEL)
    );
    runtime
        .lock_buffer(
            &mut memory,
            &audio,
            primary,
            &[primary.0, 0, 4, DATA + 8, DATA + 12, 0, 0, 0],
        )
        .unwrap();
    let data = read(&memory, DATA + 8);
    memory
        .write(GuestAddress(data), &[0, 0x40, 0, 0x40])
        .unwrap();
    runtime
        .unlock_buffer(
            &mut memory,
            &mut audio,
            primary,
            &[primary.0, data, 4, 0, 0],
        )
        .unwrap();
    runtime
        .buffer_call(
            &mut memory,
            &mut audio,
            None,
            primary,
            12,
            &[primary.0, 0, 0, 1],
        )
        .unwrap();
    let submission = audio.submit(1).unwrap();
    assert_eq!(audio.submission_pcm(submission).unwrap(), &[0.5, 0.5]);
    audio.acknowledge(submission, 1).unwrap();
    runtime
        .buffer_call(&mut memory, &mut audio, None, primary, 18, &[primary.0])
        .unwrap();
    let ObjectKind32::Buffer(primary) = &runtime.object(primary).unwrap().kind else {
        unreachable!()
    };
    assert_eq!(primary.native.positions(&audio).unwrap().0, 0);
}

#[test]
fn primary_foreground_loss_and_other_device_secondary_loss_require_restore() {
    let (mut runtime, mut memory, mut audio, device) = context();
    runtime
        .create_device(
            &mut memory,
            &mut audio,
            FactoryGate::Create8,
            &[0, DATA + 24, 0],
        )
        .unwrap();
    let other = GuestAddress(read(&memory, DATA + 24));
    runtime
        .set_cooperative_level(&mut audio, None, other, 2, 1)
        .unwrap();
    let secondary = buffer(&mut runtime, &mut memory, &mut audio, other, 0);
    write_words(
        &mut memory,
        DATA + 128,
        &[36, BufferDescription::PRIMARY, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 4, 0)
        .unwrap();
    let primary = GuestAddress(read(&memory, DATA + 4));
    runtime
        .set_cooperative_level(&mut audio, None, device, 1, 4)
        .unwrap();
    let ObjectKind32::Buffer(record) = &runtime.object(secondary).unwrap().kind else {
        unreachable!()
    };
    assert!(record.native.lost);
    assert_eq!(
        runtime.buffer_call(&mut memory, &mut audio, None, secondary, 20, &[secondary.0]),
        Err(HResult::OTHER_APP_PRIORITY)
    );
    let slot = runtime.slot(device).unwrap();
    let mut candidate = runtime.device(device).unwrap().native;
    candidate.focused = false;
    runtime
        .publish_focus_changes(&mut audio, None, &[(slot, candidate)])
        .unwrap();
    let ObjectKind32::Buffer(record) = &runtime.object(primary).unwrap().kind else {
        unreachable!()
    };
    assert!(record.native.lost);
    assert_eq!(
        runtime.buffer_call(&mut memory, &mut audio, None, primary, 20, &[primary.0]),
        Err(HResult::OTHER_APP_PRIORITY)
    );
    runtime
        .buffer_call(&mut memory, &mut audio, None, secondary, 20, &[secondary.0])
        .unwrap();
    candidate.focused = true;
    runtime
        .publish_focus_changes(&mut audio, None, &[(slot, candidate)])
        .unwrap();
    runtime
        .buffer_call(&mut memory, &mut audio, None, primary, 20, &[primary.0])
        .unwrap();
    let before = runtime.device(other).unwrap().native.cooperative_level;
    assert_eq!(
        runtime.set_cooperative_level(&mut audio, None, other, 2, 4),
        Err(HResult::ALLOCATED)
    );
    assert_eq!(
        runtime.device(other).unwrap().native.cooperative_level,
        before
    );
}
