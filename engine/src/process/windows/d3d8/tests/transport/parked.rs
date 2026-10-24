use super::*;
use crate::{
    graphics::d3d8::{device::DeviceError, resources::ResourceError},
    process::calls::call::PendingOwner,
    windows::thread::ThreadSchedule32,
};

fn parked() -> (EngineInstance, ReadbackId, SurfaceRef, Vec<u8>) {
    let (mut engine, _, _, id, surface) = front_buffer_call_with_threads(true);
    let data: Vec<u8> = (0..64 * 64 * 4).map(|index| (index % 251) as u8).collect();
    let encoded = receipt(&engine, id, ReadbackStatus::Complete, &data);
    engine.create_test_thread_at_gate(0x9000, 0x9100).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert!(engine.d3d8_reply.is_none());
    assert!(engine.pending_call.is_none());
    (engine, id, surface, encoded)
}

fn shadow(
    engine: &EngineInstance,
    id: ReadbackId,
    surface: SurfaceRef,
) -> Result<Vec<u8>, ResourceError> {
    engine
        .d3d8
        .as_ref()
        .unwrap()
        .device_by_epoch(id.epoch)
        .unwrap()
        .resources()
        .bytes(surface)
        .map(|bytes| bytes.to_vec())
}

#[test]
fn exact_lock_readback_for_a_parked_thread_is_published_only_when_its_owner_resumes() {
    let (mut context, mut memory, device) = runtime();
    context
        .device_resource_call(&mut memory, device, 25, &[device.0, 4, 4, 21, 0, 1, DATA])
        .unwrap();
    let pointer = GuestAddress(read(&memory, DATA));
    let surface = context.resource(pointer, device).unwrap();
    context
        .device_mut(device)
        .unwrap()
        .resources_mut()
        .unwrap()
        .mark_gpu_written(surface)
        .unwrap();
    while let Some(batch) = context.device_mut(device).unwrap().seal_batch().unwrap() {
        context
            .device_mut(device)
            .unwrap()
            .acknowledge_batch(batch)
            .unwrap();
    }
    memory.write(GuestAddress(DATA + 16), &[0xa5; 8]).unwrap();
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(context);
    engine
        .configure_user32(
            crate::windows::user32::User32Limits32::default(),
            PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
        )
        .unwrap();
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
    let (generation, token) = capture_gate(
        &mut engine,
        MethodGate {
            interface: InterfaceKind::Surface,
            slot: 9,
        },
        &[pointer.0, DATA + 16, 0, 0],
    );
    assert_eq!(
        engine.complete_windows_call(KEY, generation, token),
        Err(HostError::Call(CallError::Busy))
    );
    let super::super::super::runtime::ReplyKind32::LockReadback(id) =
        engine.d3d8_reply.unwrap().kind
    else {
        panic!("surface lock waits for an authentic lock readback");
    };
    let data: Vec<u8> = (0..64).map(|index| index as u8).collect();
    let encoded = receipt(&engine, id, ReadbackStatus::Complete, &data);
    engine.create_test_thread_at_gate(0x9000, 0x9100).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let active = engine.arena().to_vec();
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    engine.append_d3d8_readback_input(0, &data).unwrap();
    engine.finish_d3d8_readback_input().unwrap();
    assert_eq!(engine.arena(), active);
    assert!(engine.pending_call.is_none());
    assert_eq!(
        read(engine.memory.as_ref().unwrap(), DATA + 16),
        0xa5a5_a5a5
    );
    assert_eq!(
        read(engine.memory.as_ref().unwrap(), DATA + 20),
        0xa5a5_a5a5
    );
    assert_eq!(shadow(&engine, id, surface).unwrap(), data);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[0], state.registers[4]),
        (0x9100, 0, 0x8014)
    );
    assert_eq!(read(engine.memory.as_ref().unwrap(), DATA + 16), 16);
    let address = read(engine.memory.as_ref().unwrap(), DATA + 20);
    assert!((0x2000_0000..0x2008_0000).contains(&address));
    let mut actual = [0; 64];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(address), &mut actual)
        .unwrap();
    assert_eq!(actual.as_slice(), data);
    assert!(engine.pending_call.is_none());
    assert!(engine.d3d8_reply.is_none());
}

#[test]
fn parked_readback_rejects_wrong_payload_and_forged_owner_without_publication() {
    let (mut engine, id, surface, encoded) = parked();
    let active = engine.arena().to_vec();
    let before = shadow(&engine, id, surface);
    let owner = engine.d3d8_readback_owner(id).unwrap();
    assert_eq!(owner.thread, 1);
    let mut wrong = encoded[..64].to_vec();
    let length = u32::from_le_bytes(wrong[12..16].try_into().unwrap()) - 1;
    wrong[12..16].copy_from_slice(&length.to_le_bytes());
    wrong[8..12].copy_from_slice(&(64 + length).to_le_bytes());
    assert_eq!(
        engine.begin_d3d8_readback_input(&wrong),
        Err(HostError::InvalidRequest)
    );
    for forged in [
        super::super::super::D3D8ReadbackOwner32 {
            token: owner.token + 1,
            ..owner
        },
        super::super::super::D3D8ReadbackOwner32 { thread: 2, ..owner },
        super::super::super::D3D8ReadbackOwner32 {
            owner: PendingOwner::Core(u64::MAX),
            ..owner
        },
    ] {
        assert_eq!(
            engine.d3d8_complete_owned_readback(&encoded, forged),
            Err(HostError::D3D8(D3D8Error32::StaleReply))
        );
        assert_eq!(engine.arena(), active);
        assert_eq!(shadow(&engine, id, surface), before);
    }
    assert_eq!(engine.d3d8_readback_owner(id).unwrap(), owner);
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    engine.abort_d3d8_readback_input().unwrap();
    assert_eq!(engine.arena(), active);
    assert_eq!(shadow(&engine, id, surface), before);
}

#[test]
fn staged_parked_readback_rejects_owner_thread_exit_and_later_thread_creation() {
    let (mut engine, id, surface, encoded) = parked();
    let active = engine.arena().to_vec();
    let before = shadow(&engine, id, surface);
    let owner = engine.d3d8_readback_owner(id).unwrap();
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    engine
        .append_d3d8_readback_input(0, &encoded[64..64 + 4096])
        .unwrap();
    engine.terminate_windows_thread_for_test(1, 77).unwrap();
    engine.create_test_thread_at_gate(0x9000, 0x9100).unwrap();
    assert_eq!(engine.windows_thread_info(3).unwrap().id, 3);
    assert_eq!(
        engine.checked_d3d8_readback_owner(owner, id),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.append_d3d8_readback_input(4096, &encoded[64 + 4096..64 + 8192]),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.finish_d3d8_readback_input(),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    engine.abort_d3d8_readback_input().unwrap();
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(engine.arena(), active);
    assert_eq!(shadow(&engine, id, surface), before);
}

#[test]
fn parked_allocation_failure_resumes_its_owner_with_failure_without_touching_current_cpu() {
    let (mut engine, id, surface, _) = parked();
    let active = engine.arena().to_vec();
    let before = shadow(&engine, id, surface);
    let failed = receipt(&engine, id, ReadbackStatus::Allocation, &[]);
    engine.begin_d3d8_readback_input(&failed[..64]).unwrap();
    engine.finish_d3d8_readback_input().unwrap();
    assert_eq!(engine.arena(), active);
    assert_eq!(shadow(&engine, id, surface), before);
    assert_eq!(
        engine.d3d8_complete_readback(&failed),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[0], state.registers[4]),
        (0x9100, HResult::OUT_OF_MEMORY.0, 0x800c)
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.d3d8_reply.is_none());
}

#[test]
fn device_loss_rejects_staged_parked_bytes_and_returns_device_lost_to_the_owner() {
    let (mut engine, id, surface, encoded) = parked();
    let active = engine.arena().to_vec();
    let before = shadow(&engine, id, surface);
    engine.begin_d3d8_readback_input(&encoded[..64]).unwrap();
    engine
        .append_d3d8_readback_input(0, &encoded[64..65])
        .unwrap();
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device_by_epoch(id.epoch)
            .unwrap()
            .generation(),
        1
    );
    assert_eq!(
        engine.d3d8_observe_device(id.epoch, 2, DeviceStatus::Lost),
        Err(HostError::D3D8(D3D8Error32::Device(
            DeviceError::StaleReadback
        )))
    );
    assert_eq!(engine.arena(), active);
    engine
        .d3d8_observe_device(id.epoch, 1, DeviceStatus::Lost)
        .unwrap();
    assert_eq!(
        engine.append_d3d8_readback_input(1, &encoded[65..66]),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(
        engine.d3d8_complete_readback(&encoded),
        Err(HostError::D3D8(D3D8Error32::StaleReply))
    );
    assert_eq!(engine.arena(), active);
    assert_eq!(shadow(&engine, id, surface), before);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[0], state.registers[4]),
        (0x9100, HResult::DEVICE_LOST.0, 0x800c)
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.d3d8_reply.is_none());
}
