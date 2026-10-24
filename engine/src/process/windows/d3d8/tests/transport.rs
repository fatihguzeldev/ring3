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
    front_buffer_call_with_threads(false)
}

fn front_buffer_call_with_threads(
    threaded: bool,
) -> (EngineInstance, u32, u32, ReadbackId, SurfaceRef) {
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
    if threaded {
        engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
        engine
            .configure_synchronization(Default::default())
            .unwrap();
        engine.map(0x10000, 1, 3).unwrap();
        let primary = engine
            .threads
            .prepare_primary_thread(0x10000, PageRange::new(GuestAddress(0x8000), 1).unwrap())
            .unwrap();
        engine.threads.commit_primary_thread(primary);
    }
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

#[test]
fn exact_readback_for_a_parked_thread_preserves_the_other_threads_pending_call() {
    use crate::windows::{WindowsApi32, thread::ThreadSchedule32};

    let (mut engine, generation, token, id, surface) = front_buffer_call_with_threads(true);
    let data: Vec<u8> = (0..64 * 64 * 4).map(|index| (index % 251) as u8).collect();
    let encoded = receipt(&engine, id, ReadbackStatus::Complete, &data);
    let parent_state = engine.arena()[..STATE_SIZE].to_vec();
    let parent_exit = engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].to_vec();
    let parent_extended =
        crate::process::cpu::context::ExtendedCpuState::capture(engine.arena()).unwrap();
    let parent_owner = engine.pending_call.as_ref().unwrap().owner;

    let child_handle = engine.create_test_thread_at_gate(0x9000, 0x9100).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert!(engine.d3d8_reply.is_none());
    assert!(engine.pending_call.is_none());

    // A different real gate remains pending while the host returns the parked
    // thread's GPU bytes. Its captured CPU state must not be switched or edited.
    engine.map(0xa000, 1, 3).unwrap();
    engine.map(0xb000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0xb000, 2).unwrap();
    let gate = WindowsApi32::WaitForSingleObject.id();
    for (index, value) in [0xb000, 2, 0xb000, gate].into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let child_generation = engine.compile_with_gates(1, 1).unwrap();
    engine.write32(0xa000, 0xb100).unwrap();
    engine.write32(0xa004, child_handle).unwrap();
    engine.write32(0xa008, 33).unwrap();
    let mut state = State32 {
        eip: 0xb000,
        ..State32::default()
    };
    state.registers[4] = 0xa000;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: gate },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let child_token = engine
        .capture_windows_call(KEY, child_generation)
        .unwrap()
        .token;
    assert_ne!(child_token, token);
    let child_owner = engine.pending_call.as_ref().unwrap().owner;
    let active = engine.arena().to_vec();

    // Authenticate all receipt identities before the positive assertion. An
    // unrelated active call must never make a forged parked receipt acceptable.
    for (offset, width) in [(16, 8), (24, 8), (44, 4), (48, 8)] {
        let mut wrong = encoded[..64].to_vec();
        if width == 8 {
            let value = u64::from_le_bytes(wrong[offset..offset + 8].try_into().unwrap());
            wrong[offset..offset + 8].copy_from_slice(&(value + 1).to_le_bytes());
        } else {
            let value = u32::from_le_bytes(wrong[offset..offset + 4].try_into().unwrap());
            wrong[offset..offset + 4].copy_from_slice(&(value + 1).to_le_bytes());
        }
        assert_eq!(
            engine.begin_d3d8_readback_input(&wrong),
            Err(HostError::D3D8(D3D8Error32::StaleReply))
        );
        assert_eq!(engine.arena(), active);
        assert_eq!(engine.pending_call.as_ref().unwrap().token, child_token);
        assert_eq!(engine.pending_call.as_ref().unwrap().owner, child_owner);
    }

    // OLD active-thread-only validation fails here with StaleReply, although
    // the exact readback, pending owner/token and captured parent frame survive.
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    for (offset, chunk) in data.chunks(4096).enumerate() {
        engine
            .append_d3d8_readback_input((offset * 4096) as u32, chunk)
            .unwrap();
    }
    engine.finish_d3d8_readback_input().unwrap();
    assert_eq!(engine.arena(), active);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, child_token);
    assert_eq!(engine.pending_call.as_ref().unwrap().owner, child_owner);
    assert!(engine.d3d8_reply.is_none());
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
        engine.begin_d3d8_readback_input(&encoded[..64]),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(engine.arena(), active);

    engine
        .complete_call(KEY, child_generation, child_token, 0)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let resumed = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let mut expected = decode_state(&parent_state).unwrap();
    expected.eip = 0x9100;
    expected.registers[0] = 0;
    expected.registers[4] += 12;
    assert_eq!(resumed, expected);
    assert!(parent_extended.matches(engine.arena()));
    assert!(engine.pending_call.is_none());
    assert!(engine.d3d8_reply.is_none());
    assert_eq!(
        parent_owner,
        crate::process::calls::call::PendingOwner::Replacement(generation)
    );
    assert_eq!(
        crate::abi::x86::decode_exit(&parent_exit).unwrap().reason,
        ExitReason::Gate {
            id: MethodGate {
                interface: InterfaceKind::Device,
                slot: 30
            }
            .id()
        }
    );
}

mod parked;

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
