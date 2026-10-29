use super::super::{
    DispatchError32,
    lifetime::LifetimeChange32,
    tests::support::{DATA, read, runtime, texture},
};
use super::holds::validate_delta;
use crate::{
    graphics::d3d8::api::{HResult, InterfaceKind},
    memory::{GuestAddress, PageRange, Permissions},
};

fn held(runtime: &super::super::D3D8Runtime32, device: GuestAddress) -> Vec<GuestAddress> {
    let super::super::objects::ObjectKind32::Device(record) = &runtime.object(device).unwrap().kind
    else {
        unreachable!()
    };
    record.resource_holds.clone()
}

fn skips(runtime: &super::super::D3D8Runtime32, device: GuestAddress) -> u64 {
    let super::super::objects::ObjectKind32::Device(record) = &runtime.object(device).unwrap().kind
    else {
        unreachable!()
    };
    record.binding_lifetime_skips
}

#[test]
fn same_binding_updates_stream_stride_and_index_base_without_replacing_holds() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 64, 0, 2, 1, DATA])
        .unwrap();
    let vertex = GuestAddress(read(&memory, DATA));
    runtime
        .device_resource_call(
            &mut memory,
            device,
            24,
            &[device.0, 12, 0, 101, 2, DATA + 4],
        )
        .unwrap();
    let index = GuestAddress(read(&memory, DATA + 4));
    runtime
        .binding_call(&mut memory, device, 83, &[device.0, 0, vertex.0, 12])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 85, &[device.0, index.0, 2])
        .unwrap();
    let holds = held(&runtime, device);
    let references = (
        runtime.com.state(vertex).unwrap(),
        runtime.com.state(index).unwrap(),
    );
    let revision = runtime.device(device).unwrap().state().revision();
    runtime
        .binding_call(&mut memory, device, 83, &[device.0, 0, vertex.0, 20])
        .unwrap();
    assert_eq!(
        held(&runtime, device),
        holds,
        "an unchanged lifetime must retain the existing ownership ledger"
    );
    runtime
        .binding_call(&mut memory, device, 85, &[device.0, index.0, 7])
        .unwrap();
    let state = runtime.device(device).unwrap().state();
    assert_eq!(state.snapshot().streams[0].stride, 20);
    assert_eq!(state.snapshot().base_vertex, 7);
    assert_eq!(state.revision(), revision + 2);
    assert_eq!(
        (
            runtime.com.state(vertex).unwrap(),
            runtime.com.state(index).unwrap()
        ),
        references
    );
    assert_eq!(held(&runtime, device), holds);
    assert_eq!(skips(&runtime, device), 2);
}

#[test]
fn same_binding_canonicalizes_interface_aliases_but_keeps_recording_and_com_checks() {
    let (mut runtime, mut memory, device) = runtime();
    let first = texture(&mut runtime, &mut memory, device);
    let second = texture(&mut runtime, &mut memory, device);
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 0, first.0])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 1, second.0])
        .unwrap();
    memory
        .write(GuestAddress(DATA + 64), &InterfaceKind::BaseTexture.iid().0)
        .unwrap();
    runtime
        .query_interface(&mut memory, first, DATA + 64, DATA + 80)
        .unwrap();
    let alias = GuestAddress(read(&memory, DATA + 80));
    assert_eq!(
        alias, first,
        "D3D8 exposes supported base interfaces through the canonical guest pointer"
    );
    let holds = held(&runtime, device);
    let references = runtime.com.state(first).unwrap();
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 0, alias.0])
        .unwrap();
    assert_eq!(held(&runtime, device), holds);
    assert_eq!(runtime.com.state(first).unwrap(), references);
    assert_eq!(skips(&runtime, device), 1);
    assert!(matches!(
        runtime.binding_call(&mut memory, device, 83, &[device.0, 0, alias.0, 12]),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert_eq!(held(&runtime, device), holds);
    runtime
        .binding_call(&mut memory, device, 52, &[device.0])
        .unwrap();
    runtime
        .binding_call(&mut memory, device, 61, &[device.0, 0, alias.0])
        .unwrap();
    assert_eq!(
        runtime.com.state(first).unwrap().holds,
        references.holds + 1
    );
    runtime
        .binding_call(&mut memory, device, 53, &[device.0, DATA + 16])
        .unwrap();
    assert!(!runtime.device(device).unwrap().state().is_recording());
    let before = held(&runtime, device);
    memory
        .protect(
            PageRange::new(GuestAddress(first.0 & !4095), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert!(matches!(
        runtime.binding_call(&mut memory, device, 61, &[device.0, 0, alias.0]),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert_eq!(held(&runtime, device), before);
    assert_eq!(skips(&runtime, device), 1);
}

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
