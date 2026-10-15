use super::support::{DATA, read, runtime};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        graphics::d3d8::{ReadbackReceipt, ReadbackStatus, encode_readback_receipt},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    graphics::d3d8::{
        api::{AdapterProfile32, DisplayMode32, HResult, InterfaceKind, MethodGate},
        commands::ReadbackId,
        device::DeviceStatus,
        resources::{Format, SurfaceRef},
    },
    memory::{GuestAddress, PageRange},
    process::{CallError, EngineInstance, HostError, windows::d3d8::D3D8Error32},
};

const KEY: u64 = 91;

fn front_buffer_call() -> (EngineInstance, u32, u32, ReadbackId, SurfaceRef) {
    let (mut context, memory, device) = runtime();
    let mode = DisplayMode32 {
        width: 64,
        height: 64,
        refresh_rate: 60,
        format: Format::X8R8G8B8,
    };
    context.config.adapter = AdapterProfile32::new(mode, vec![mode]).unwrap();
    let mut memory = memory;
    context
        .device_resource_call(&mut memory, device, 27, &[device.0, 64, 64, 21, DATA])
        .unwrap();
    let destination = GuestAddress(read(&memory, DATA));
    let surface = context.resource(destination, device).unwrap();
    while let Some(id) = context.device_mut(device).unwrap().seal_batch().unwrap() {
        context
            .device_mut(device)
            .unwrap()
            .acknowledge_batch(id)
            .unwrap();
    }
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(context);
    engine
        .configure_user32(
            crate::windows::user32::User32Limits32::default(),
            PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
        )
        .unwrap();
    let (generation, token) = capture_gate(
        &mut engine,
        MethodGate {
            interface: InterfaceKind::Device,
            slot: 30,
        },
        &[device.0, destination.0],
    );
    assert_eq!(
        engine.complete_windows_call(KEY, generation, token),
        Err(HostError::Call(CallError::Busy))
    );
    let super::super::runtime::ReplyKind32::Readback(id) = engine.d3d8_reply.unwrap().kind else {
        panic!("front buffer waits for its exact readback")
    };
    (engine, generation, token, id, surface)
}

pub(super) fn capture_gate(
    engine: &mut EngineInstance,
    method: MethodGate,
    arguments: &[u32],
) -> (u32, u32) {
    engine.map(0x8000, 1, 3).unwrap();
    engine.map(0x9000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x9000, 2).unwrap();
    let gate = method.id();
    for (index, value) in [0x9000, 2, 0x9000, gate].into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    for (index, value) in std::iter::once(0x9100)
        .chain(arguments.iter().copied())
        .enumerate()
    {
        engine.write32(0x8000 + index as u32 * 4, value).unwrap();
    }
    let mut state = State32 {
        eip: 0x9000,
        ..State32::default()
    };
    state.registers[4] = 0x8000;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: gate },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    (generation, token)
}

fn receipt(
    engine: &EngineInstance,
    id: ReadbackId,
    status: ReadbackStatus,
    bytes: &[u8],
) -> Vec<u8> {
    let source = engine
        .d3d8
        .as_ref()
        .unwrap()
        .device_by_epoch(id.epoch)
        .unwrap()
        .pending_readback_details(id)
        .unwrap()
        .2;
    let mut output = vec![0; 64 + bytes.len()];
    encode_readback_receipt(
        ReadbackReceipt {
            id,
            source,
            status,
            bytes,
        },
        &mut output,
    )
    .unwrap();
    output
}

#[test]
fn chunked_readback_keeps_guest_frame_and_shadow_pending_until_exact_complete_receipt() {
    let (mut engine, generation, token, id, surface) = front_buffer_call();
    let data: Vec<u8> = (0..64 * 64 * 4).map(|index| (index % 251) as u8).collect();
    let encoded = receipt(&engine, id, ReadbackStatus::Complete, &data);
    let mut wrong = encoded[..64].to_vec();
    wrong[24..32].copy_from_slice(&(id.sequence + 1).to_le_bytes());
    assert_eq!(
        engine.begin_d3d8_readback_input(&wrong),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.complete_call(KEY, generation, token, 0),
        Err(HostError::Call(CallError::Busy))
    );
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    assert_eq!(
        engine.append_d3d8_readback_input(1, &data[..1]),
        Err(HostError::InvalidRequest)
    );
    engine.append_d3d8_readback_input(0, &data[..4096]).unwrap();
    assert_eq!(
        engine.finish_d3d8_readback_input(),
        Err(HostError::InvalidRequest)
    );
    assert!(engine.pending_call.is_some());
    for offset in (4096..data.len()).step_by(4096) {
        engine
            .append_d3d8_readback_input(
                offset as u32,
                &data[offset..(offset + 4096).min(data.len())],
            )
            .unwrap();
    }
    engine.finish_d3d8_readback_input().unwrap();
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device_by_epoch(id.epoch)
            .unwrap()
            .resources()
            .bytes(surface)
            .unwrap(),
        data
    );
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[0], state.registers[4]),
        (0x9100, 0, 0x800c)
    );
    assert!(engine.pending_call.is_none());
}

#[test]
fn host_allocation_failure_finishes_readback_with_failure_and_rejects_late_receipt() {
    let (mut engine, generation, token, id, _) = front_buffer_call();
    let encoded = receipt(&engine, id, ReadbackStatus::Allocation, &[]);
    engine.d3d8_complete_readback(&encoded).unwrap();
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        HResult::OUT_OF_MEMORY.0
    );
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
}

#[test]
fn device_loss_invalidates_staged_receipt_and_completes_owning_frame_with_device_lost() {
    let (mut engine, generation, token, id, _) = front_buffer_call();
    let encoded = receipt(&engine, id, ReadbackStatus::Complete, &vec![0; 64 * 64 * 4]);
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    engine
        .d3d8_observe_device(id.epoch, 1, DeviceStatus::Lost)
        .unwrap();
    assert_eq!(
        engine.append_d3d8_readback_input(0, &encoded[64..65]),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    engine.abort_d3d8_readback_input().unwrap();
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        HResult::DEVICE_LOST.0
    );
    engine.close();
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::Closed)
    );
}
