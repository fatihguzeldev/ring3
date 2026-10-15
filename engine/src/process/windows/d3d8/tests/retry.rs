use super::{
    support::{DATA, read, runtime},
    transport::capture_gate,
};
use crate::{
    abi::x86::{STATE_SIZE, decode_state},
    graphics::d3d8::{
        api::{InterfaceKind, MethodGate},
        device::{Device, DeviceStatus},
        resources::ResourceError,
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, windows::d3d8::runtime::ReplyKind32},
};

const KEY: u64 = 91;

fn drain(engine: &mut EngineInstance, epoch: u64) {
    while let Some(id) = engine.d3d8_seal_batch(epoch).unwrap() {
        engine.d3d8_acknowledge_batch(id).unwrap();
    }
}

#[test]
fn device_loss_does_not_replace_an_unaccepted_release_retry_with_a_hresult_refcount() {
    let (mut context, mut memory, device) = runtime();
    context
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let resource = context.resource(buffer, device).unwrap().resource;
    let packet = context
        .device_mut(device)
        .unwrap()
        .seal_batch()
        .unwrap()
        .unwrap();
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(context);
    let (generation, token) = capture_gate(
        &mut engine,
        MethodGate {
            interface: InterfaceKind::VertexBuffer,
            slot: 2,
        },
        &[buffer.0],
    );
    assert_eq!(
        engine.complete_windows_call(KEY, generation, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert!(matches!(
        engine.d3d8_reply.unwrap().kind,
        ReplyKind32::Retry
    ));
    assert_eq!(engine.d3d8.as_ref().unwrap().com.references(buffer), Ok(1));
    engine.d3d8_acknowledge_batch(packet).unwrap();
    engine
        .d3d8_observe_device(packet.epoch, 1, DeviceStatus::Lost)
        .unwrap();
    assert!(matches!(
        engine.d3d8_reply.unwrap().kind,
        ReplyKind32::Retry
    ));
    drain(&mut engine, packet.epoch);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert!(engine.d3d8.as_ref().unwrap().object(buffer).is_err());
    assert_eq!(engine.d3d8.as_ref().unwrap().com.references(device), Ok(1));
    drain(&mut engine, packet.epoch);
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device_by_epoch(packet.epoch)
            .unwrap()
            .resources()
            .desc(resource),
        Err(ResourceError::StaleResource)
    );
}

#[test]
fn device_loss_preserves_accepted_upload_completion_and_never_replays_unlock() {
    let (mut context, mut memory, device) = runtime();
    let description = context.device(device).unwrap().description();
    let mut limits = context.config.device;
    limits.commands.batch_bytes = 512;
    *context.device_mut(device).unwrap() = Device::new(1, description, limits).unwrap();
    while let Some(id) = context.device_mut(device).unwrap().seal_batch().unwrap() {
        context
            .device_mut(device)
            .unwrap()
            .acknowledge_batch(id)
            .unwrap();
    }
    context
        .device_resource_call(&mut memory, device, 23, &[device.0, 4096, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let resource = context.resource(buffer, device).unwrap().resource;
    context
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 0, 4096, DATA + 4, 0],
        )
        .unwrap();
    let staging = GuestAddress(read(&memory, DATA + 4));
    memory.write(staging, &[0x3a; 4096]).unwrap();
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(context);
    let (generation, token) = capture_gate(
        &mut engine,
        MethodGate {
            interface: InterfaceKind::VertexBuffer,
            slot: 12,
        },
        &[buffer.0],
    );
    assert_eq!(
        engine.complete_windows_call(KEY, generation, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert!(matches!(
        engine.d3d8_reply.unwrap().kind,
        ReplyKind32::Drain(0)
    ));
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(device)
            .unwrap()
            .resources()
            .version(resource),
        Ok(2)
    );
    drain(&mut engine, 1);
    engine
        .d3d8_observe_device(1, 1, DeviceStatus::Lost)
        .unwrap();
    assert!(matches!(
        engine.d3d8_reply.unwrap().kind,
        ReplyKind32::Drain(0)
    ));
    drain(&mut engine, 1);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(device)
            .unwrap()
            .resources()
            .version(resource),
        Ok(2)
    );
    assert!(engine.pending_call.is_none());
}
