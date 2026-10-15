use super::super::{
    DispatchError32,
    lifetime::LifetimeChange32,
    tests::support::{DATA, read, runtime, texture},
};
use super::holds::validate_delta;
use crate::{graphics::d3d8::api::HResult, memory::GuestAddress};

#[test]
fn binding_hold_delta_preserves_duplicate_counts_and_rejects_overrelease() {
    let mut held = Vec::with_capacity(3);
    held.extend([GuestAddress(7), GuestAddress(7)]);
    assert_eq!(
        validate_delta(
            &held,
            &[GuestAddress(7)],
            &[GuestAddress(9), GuestAddress(9)]
        ),
        Ok(())
    );
    assert_eq!(
        validate_delta(&held, &[GuestAddress(7); 3], &[]),
        Err(HResult::INVALID_CALL)
    );
    assert_eq!(
        validate_delta(&held, &[], &[GuestAddress(9); 2]),
        Err(HResult::OUT_OF_MEMORY)
    );
}

#[test]
fn recorded_texture_holds_survive_external_zero_and_get_revives_owned_reference() {
    let (mut runtime, mut memory, device) = runtime();
    let texture = texture(&mut runtime, &mut memory, device);
    runtime
        .binding_call(&mut memory, device, 52, &[device.0])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 0, texture.0])
        .unwrap();
    assert_eq!(
        runtime.device(device).unwrap().state().snapshot().textures[0],
        0
    );
    assert_eq!(runtime.com.state(texture).unwrap().holds, 1);
    runtime
        .change_lifetime(
            &mut memory,
            LifetimeChange32 {
                release: vec![texture],
                ..Default::default()
            },
            &[],
        )
        .unwrap();
    assert_eq!(runtime.com.references(texture).unwrap(), 0);
    runtime
        .binding_call(&mut memory, device, 53, &[device.0, DATA + 16])
        .unwrap();
    let block = read(&memory, DATA + 16);
    runtime
        .binding_call(&mut memory, device, 54, &[device.0, block])
        .unwrap();
    assert_eq!(runtime.com.state(texture).unwrap().holds, 2);
    runtime
        .binding_call(&mut memory, device, 60, &[device.0, 0, DATA + 20])
        .unwrap();
    assert_eq!(read(&memory, DATA + 20), texture.0);
    assert_eq!(runtime.com.references(texture).unwrap(), 1);
    runtime
        .binding_call(&mut memory, device, 56, &[device.0, block])
        .unwrap();
    assert_eq!(runtime.com.state(texture).unwrap().holds, 1);
}

#[test]
fn failed_end_state_block_output_preserves_recording_and_resource_holds() {
    let (mut runtime, mut memory, device) = runtime();
    let texture = texture(&mut runtime, &mut memory, device);
    runtime
        .binding_call(&mut memory, device, 52, &[device.0])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 0, texture.0])
        .unwrap();
    assert!(matches!(
        runtime.binding_call(&mut memory, device, 53, &[device.0, 0x8000]),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert!(runtime.device(device).unwrap().state().is_recording());
    assert_eq!(runtime.com.state(texture).unwrap().holds, 1);
    runtime
        .binding_call(&mut memory, device, 53, &[device.0, DATA + 16])
        .unwrap();
    assert!(!runtime.device(device).unwrap().state().is_recording());
    assert_eq!(runtime.com.state(texture).unwrap().holds, 1);
}

#[test]
fn get_stream_publishes_pointer_stride_and_addref_as_one_guest_transaction() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 64, 0, 2, 1, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    runtime
        .binding_call(&mut memory, device, 83, &[device.0, 0, buffer.0, 12])
        .unwrap();
    memory
        .write(GuestAddress(DATA + 4), &0x55aa_u32.to_le_bytes())
        .unwrap();
    assert!(matches!(
        runtime.binding_call(&mut memory, device, 84, &[device.0, 0, DATA + 4, 0x8000]),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert_eq!(read(&memory, DATA + 4), 0x55aa);
    assert_eq!(runtime.com.references(buffer).unwrap(), 1);
    runtime
        .binding_call(&mut memory, device, 84, &[device.0, 0, DATA + 4, DATA + 8])
        .unwrap();
    assert_eq!(read(&memory, DATA + 4), buffer.0);
    assert_eq!(read(&memory, DATA + 8), 12);
    assert_eq!(runtime.com.references(buffer).unwrap(), 2);
    runtime
        .binding_call(&mut memory, device, 83, &[device.0, 0, 0, 0])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 84, &[device.0, 0, DATA + 4, DATA + 8])
        .unwrap();
    assert_eq!(read(&memory, DATA + 4), 0);
    assert_eq!(read(&memory, DATA + 8), 12);
}
