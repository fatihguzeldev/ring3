use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v7, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{EventOrigin32, ExceptionCause32, State32},
    },
    memory::{GuestAddress, PAGE_SIZE, PageRange, Permissions},
    process::{CallError, HostError, cpu::events::detection::DetectedEvent32},
};

const KEY: u64 = 0x52454144;

pub(in crate::process) fn fixture(resident: bool) -> (EngineInstance, PendingOwner) {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    let owner = if resident {
        let id = engine.compile_resident(1).unwrap().get();
        engine.guard_material_resident(KEY, id).unwrap();
        PendingOwner::Resident(id)
    } else {
        engine.compile(1).unwrap();
        engine.guard_material(KEY, engine.generation()).unwrap();
        PendingOwner::Replacement(engine.generation())
    };
    (engine, owner)
}

fn stop(engine: &mut EngineInstance, reason: ExitReason) {
    let state = State32 {
        eip: 0x1000,
        eflags: 2,
        ..Default::default()
    };
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v7(
        &ExecutionExit { retired: 0, reason },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

#[test]
fn fixed_wasm_layout_uses_canonical_readonly_arrays_without_arena_publication() {
    use std::mem::{offset_of, size_of};
    assert_eq!(size_of::<CpuRead32View32>(), 24);
    assert_eq!(
        [
            offset_of!(CpuRead32View32, active_epoch),
            offset_of!(CpuRead32View32, mappings),
            offset_of!(CpuRead32View32, permissions),
            offset_of!(CpuRead32View32, backing),
            offset_of!(CpuRead32View32, resident_pages)
        ],
        [0, 8, 12, 16, 20]
    );
    assert_eq!(size_of::<Permissions>(), 1);
    let (mut engine, owner) = fixture(true);
    assert_eq!(engine.cpu_read32_view_address(1), 0);
    let before = engine.arena().to_vec();
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let address = engine.cpu_read32_view_address(epoch);
    assert_ne!(address, 0);
    let (mappings, permissions, backing) = engine.memory().unwrap().cpu_read_arrays();
    assert_eq!(
        (mappings.len(), permissions.len(), backing.len()),
        (1 << 20, 1 << 20, 5 * PAGE_SIZE as usize)
    );
    assert_eq!(
        (
            engine.cpu_read32_view.mappings,
            engine.cpu_read32_view.permissions,
            engine.cpu_read32_view.backing
        ),
        (
            mappings.as_ptr() as usize as u32,
            permissions.as_ptr() as usize as u32,
            backing.as_ptr() as usize as u32
        )
    );
    assert_eq!(engine.cpu_read32_view.resident_pages, 5);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.cpu_read32_view_address(0), 0);
    assert_eq!(engine.cpu_read32_view_address(epoch + 1), 0);
    assert_eq!(engine.cpu_read32_view_address(epoch), address);
}

#[test]
fn only_resident_active_epochs_open_and_failed_transitions_leave_projection_unchanged() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        assert_eq!(
            engine.cpu_read32_view.active_epoch,
            if resident { epoch } else { 0 }
        );
        let before = engine.cpu_read32_view;
        assert_eq!(
            engine.discard_exception_detection(epoch + 1),
            Err(HostError::Call(CallError::InvalidToken))
        );
        assert_eq!(engine.cpu_read32_view, before);
        engine.detection_epoch = i64::MAX as u64;
        assert_eq!(
            engine.begin_exception_detection(owner),
            Err(HostError::GenerationExhausted)
        );
        assert_eq!(engine.cpu_read32_view, before);
        engine.close();
        assert_eq!(engine.cpu_read32_view.active_epoch, 0);
        assert_eq!(engine.cpu_read32_view_address(epoch), 0);
        engine.clear_certified_page_fault32();
        assert_eq!(engine.cpu_read32_view.active_epoch, 0);
    }
}

#[test]
fn real_fault_revokes_until_native_access_clears_the_actual_witness() {
    let (mut engine, owner) = fixture(true);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let address = engine.cpu_read32_view_address(epoch);
    engine.cpu_read32(0x6001).unwrap();
    assert!(engine.certified_page_fault32(epoch).is_some());
    assert_eq!(engine.cpu_read32_view.active_epoch, 0);
    let fault_arena = engine.arena().to_vec();
    let fault_sequence = engine.cpu_page_fault_sequence32();
    assert_eq!(engine.cpu_read32_view_address(epoch), 0);
    assert_eq!(
        engine.arena(),
        fault_arena,
        "query cannot clear a real witness"
    );
    assert_eq!(engine.cpu_page_fault_sequence32(), fault_sequence);
    engine.cpu_read32(0x7001).unwrap();
    assert_eq!(engine.certified_page_fault32(epoch), None);
    assert_eq!(engine.cpu_read32_view_address(epoch), address);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x6001)));
    assert_eq!(engine.cpu_page_fault_sequence32(), fault_sequence);
}

#[test]
fn ready_returned_idle_and_replaced_epochs_never_reuse_old_availability() {
    let (mut engine, owner) = fixture(true);
    let first = engine.begin_exception_detection(owner).unwrap();
    let second = engine.begin_exception_detection(owner).unwrap();
    assert_eq!(engine.cpu_read32_view_address(first), 0);
    assert_ne!(engine.cpu_read32_view_address(second), 0);
    stop(&mut engine, ExitReason::InvalidOpcode);
    engine
        .finish_exception_detection(
            second,
            DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::InvalidOpcode)),
            None,
        )
        .unwrap();
    assert_eq!(engine.cpu_read32_view.active_epoch, 0);
    engine.consume_exception_detection(second).unwrap();
    assert_eq!(engine.cpu_read32_view.active_epoch, 0);
    let third = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::NeedCode);
    engine
        .finish_generated_detection(third, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(engine.cpu_read32_view_address(third), 0);
    let fourth = engine.begin_exception_detection(owner).unwrap();
    engine.discard_exception_detection(fourth).unwrap();
    assert_eq!(engine.cpu_read32_view.active_epoch, 0);
}

#[test]
fn alias_cow_and_protection_change_cells_without_replacing_canonical_arrays() {
    let (mut engine, owner) = fixture(true);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let descriptor = engine.cpu_read32_view;
    let memory = engine.memory.as_mut().unwrap();
    let shared = memory
        .allocate_shared_pages(1, &0x89abcdefu32.to_le_bytes())
        .unwrap();
    for (page, cow) in [(0x9000, false), (0xa000, true)] {
        memory
            .map_shared_pages(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                &shared,
                0,
                Permissions::READ_WRITE,
                cow,
            )
            .unwrap();
    }
    let before = memory.cpu_read_arrays().0[0xa000 / PAGE_SIZE as usize];
    memory
        .cpu_write(GuestAddress(0xa000), &0x12345678u32.to_le_bytes())
        .unwrap();
    let (mappings, permissions, backing) = memory.cpu_read_arrays();
    assert_ne!(mappings[0xa000 / PAGE_SIZE as usize], before);
    assert_eq!(
        u32::from_le_bytes(
            backing[(mappings[9] as usize - 1) * PAGE_SIZE as usize..][..4]
                .try_into()
                .unwrap()
        ),
        0x89abcdef
    );
    assert_eq!(
        u32::from_le_bytes(
            backing[(mappings[10] as usize - 1) * PAGE_SIZE as usize..][..4]
                .try_into()
                .unwrap()
        ),
        0x12345678
    );
    assert_eq!(permissions[9], Permissions::READ_WRITE);
    engine.protect(0x9000, 1, 4).unwrap();
    assert_eq!(
        engine.memory().unwrap().cpu_read_arrays().1[9],
        Permissions::EXECUTE
    );
    engine.unmap(0x9000, 1).unwrap();
    assert_eq!(engine.memory().unwrap().cpu_read_arrays().0[9], 0);
    engine.cpu_read32(0x7000).unwrap();
    assert_eq!(engine.cpu_read32_view, descriptor);
    assert_ne!(engine.cpu_read32_view_address(epoch), 0);
}
