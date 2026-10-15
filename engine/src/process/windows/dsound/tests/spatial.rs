use super::{
    super::{memory::write_words, objects::ObjectKind32},
    support::*,
};
use crate::{
    audio::dsound::{BufferDescription, InterfaceKind},
    memory::GuestAddress,
};

#[test]
fn software_max_distance_stops_after_deferred_commit_and_equal_distance_only_mutes() {
    let (mut runtime, mut memory, mut audio, device) = context();
    let pointer = buffer(
        &mut runtime,
        &mut memory,
        &mut audio,
        device,
        BufferDescription::CONTROL_3D
            | BufferDescription::MUTE_AT_MAX_DISTANCE
            | BufferDescription::NOTIFY,
    );
    let slot = runtime.slot(pointer).unwrap();
    let ObjectKind32::Buffer(buffer) = &runtime.records[slot].as_ref().unwrap().kind else {
        unreachable!()
    };
    let voice = buffer.native.voice;
    let mut sync = sync();
    let (event, _) = sync.create_event(false, false, false, None).unwrap();
    write_words(&mut memory, DATA + 384, &[u32::MAX, event]).unwrap();
    runtime
        .set_notifications(&memory, &audio, &mut sync, pointer, 1, DATA + 384)
        .unwrap();
    runtime
        .spatial_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            InterfaceKind::Buffer3D,
            16,
            &[pointer.0, 1f32.to_bits(), 0],
        )
        .unwrap();
    runtime
        .buffer_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            12,
            &[pointer.0, 0, 0, 1],
        )
        .unwrap();
    runtime
        .spatial_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            InterfaceKind::Buffer3D,
            19,
            &[pointer.0, 2f32.to_bits(), 0, 0, 1],
        )
        .unwrap();
    assert!(audio.voice_state(voice).unwrap().playing);
    assert_eq!(sync.event_is_signaled(event), Ok(false));
    write_words(
        &mut memory,
        DATA + 128,
        &[
            36,
            BufferDescription::PRIMARY | BufferDescription::CONTROL_3D,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 8, 0)
        .unwrap();
    let primary = GuestAddress(read(&memory, DATA + 8));
    let listener = runtime
        .target(&memory, primary, InterfaceKind::Listener)
        .unwrap();
    runtime
        .spatial_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            listener,
            InterfaceKind::Listener,
            17,
            &[listener.0],
        )
        .unwrap();
    assert!(!audio.voice_state(voice).unwrap().playing);
    assert_eq!(sync.event_is_signaled(event), Ok(true));
    sync.reset_event(event).unwrap();
    runtime
        .spatial_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            InterfaceKind::Buffer3D,
            19,
            &[pointer.0, 1f32.to_bits(), 0, 0, 0],
        )
        .unwrap();
    runtime
        .buffer_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            12,
            &[pointer.0, 0, 0, 1],
        )
        .unwrap();
    assert!(audio.voice_state(voice).unwrap().playing);
    assert_eq!(audio.voice_controls(voice).unwrap().gains, [0.0; 2]);
    assert_eq!(sync.event_is_signaled(event), Ok(false));
    runtime
        .spatial_call(
            &mut memory,
            &mut audio,
            Some(&mut sync),
            pointer,
            InterfaceKind::Buffer3D,
            19,
            &[pointer.0, 2f32.to_bits(), 0, 0, 0],
        )
        .unwrap();
    assert!(!audio.voice_state(voice).unwrap().playing);
    assert_eq!(sync.event_is_signaled(event), Ok(true));
}
