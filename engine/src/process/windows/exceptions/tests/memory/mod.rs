mod guard;
mod heap;
mod stack;

use super::{engine, returned};
use crate::{
    abi::{arena::*, x86::*},
    cpu::{
        ExecutionExit, ExitReason,
        x86::{CpuPagingError32, PagingAccess32},
    },
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::memory::MemoryApi32,
};

const KEY: u64 = 77;

fn capture(engine: &mut EngineInstance, api: MemoryApi32, args: &[u32]) -> (u32, u32) {
    capture_id(engine, api.id(), args)
}

fn capture_id(engine: &mut EngineInstance, id: u32, args: &[u32]) -> (u32, u32) {
    let gate = *engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == id)
        .unwrap();
    for (index, word) in [gate.entry.0, 2, gate.entry.0, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate.entry.0;
    state.registers[4] -= 64;
    for (index, word) in std::iter::once(0x401000u32)
        .chain(args.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(state.registers[4] + index as u32 * 4),
                &word.to_le_bytes(),
            )
            .unwrap();
    }
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    (generation, token)
}

fn call(engine: &mut EngineInstance, api: MemoryApi32, args: &[u32]) -> Result<u32, HostError> {
    let (generation, token) = capture(engine, api, args);
    engine.complete_windows_call(KEY, generation, token)?;
    Ok(decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0])
}

fn permission_event(
    engine: &mut EngineInstance,
    address: u32,
    access: PagingAccess32,
    pc: u32,
) -> (u64, u64) {
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let unit = engine.compile_stopped_resident_entry(KEY).unwrap().get();
    engine.guard_material_resident(KEY, unit).unwrap();
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(unit))
        .unwrap();
    let CpuPagingError32::PageFault(fault) = engine
        .preflight_cpu_paging32(GuestAddress(address), 1, access)
        .unwrap_err()
    else {
        panic!("guard must reach the real certified paging boundary")
    };
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    let kind = match access {
        PagingAccess32::Read => Access::Read,
        PagingAccess32::Write => Access::Write,
        PagingAccess32::Execute => Access::Execute,
    };
    encode_exit_v13(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::PageFault {
                error_code: fault.error_code(),
                linear: fault.linear(),
                access: kind,
                length: 1,
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .finish_generated_detection(
            epoch,
            [
                23,
                fault.error_code(),
                address,
                match kind {
                    Access::Read => 1,
                    Access::Write => 2,
                    Access::Execute => 3,
                },
                1,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
        )
        .unwrap();
    (unit, epoch)
}
