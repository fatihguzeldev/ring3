use super::*;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{ModuleRuntimeConfig32, core_numeric_tests::NumericBank},
    windows::WindowsApi32,
};

const KEY: u64 = 0x434f5245;

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module(
            "core.exe",
            include_bytes!(
                "../../../../tests/process/images/startup/imports/fixtures/p2-c-imports/program.exe"
            ),
        )
        .unwrap();
    engine
        .start_module_process(
            "core.exe",
            None,
            ModuleRuntimeConfig32 {
                stack_base: 0x7e100000,
                stack_pages: 8,
                runtime_base: 0x7d000000,
            },
        )
        .unwrap();
    engine.enable_core_execution().unwrap();
    assert!(engine.artifact.is_none());
    assert!(engine.resident.is_none());
    engine
}

fn state(engine: &EngineInstance) -> State32 {
    decode_state(&engine.arena()[..STATE_SIZE]).unwrap()
}

fn gate(engine: &EngineInstance, id: u32) -> u32 {
    engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0
}

fn publish(
    engine: &mut EngineInstance,
    state: State32,
    reason: ExitReason,
) -> Result<(), HostError> {
    let banks = NumericBank::from_instance(engine)?.architectural_state();
    let prepared = engine.prepare_core_exit(
        state,
        ExecutionExit { retired: 0, reason },
        0,
        None,
        None,
        banks,
    )?;
    engine.commit_core_exit(prepared);
    Ok(())
}

fn call(engine: &mut EngineInstance, api: WindowsApi32, arguments: &[u32]) -> u32 {
    let mut current = state(engine);
    current.eip = gate(engine, api.id());
    current.registers[4] -= 64;
    let stack = current.registers[4];
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(stack), &0x401000u32.to_le_bytes())
        .unwrap();
    for (i, value) in arguments.iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(stack + 4 + i as u32 * 4), &value.to_le_bytes())
            .unwrap();
    }
    publish(engine, current, ExitReason::Gate { id: api.id() }).unwrap();
    let token = engine.capture_core_windows_call().unwrap().token;
    engine.complete_core_windows_call(token).unwrap();
    token
}

#[test]
fn windows_calls_need_no_compiled_unit_or_execution_receipt() {
    let mut engine = fixture();
    let token = call(&mut engine, WindowsApi32::SetLastError, &[0xf1234567]);
    assert!(engine.complete_core_windows_call(token).is_err());
    call(&mut engine, WindowsApi32::GetLastError, &[]);
    assert_eq!(state(&engine).registers[0], 0xf1234567);
    assert_eq!(state(&engine).eip, 0x401000);
    assert!(engine.artifact.is_none());
    assert!(engine.resident.is_none());
}

#[test]
fn core_callbacks_restore_outer_state_and_allow_nested_windows_calls() {
    let mut engine = fixture();
    let mut current = state(&engine);
    current.eip = gate(&engine, WindowsApi32::SetLastError.id());
    current.registers[4] -= 128;
    let stack = current.registers[4];
    for (offset, value) in [(0, 0x401000u32), (4, 0x12345678)] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(stack + offset), &value.to_le_bytes())
            .unwrap();
    }
    publish(
        &mut engine,
        current,
        ExitReason::Gate {
            id: WindowsApi32::SetLastError.id(),
        },
    )
    .unwrap();
    let outer = engine.capture_core_windows_call().unwrap();
    let before = engine.arena()[..LEGACY_ARENA_SIZE].to_vec();
    let return_pc = gate(&engine, 0xffffe);
    let callback = engine
        .begin_provider_callback(KEY, 0, outer.token, 0x401000, return_pc, 0xffffe, &[7, 9])
        .unwrap();
    assert!(engine.callback.as_ref().unwrap().is_core());
    call(&mut engine, WindowsApi32::GetLastError, &[]);
    let mut returned = state(&engine);
    returned.eip = return_pc;
    returned.registers[4] = stack;
    returned.registers[0] = 0xabcdef01;
    publish(&mut engine, returned, ExitReason::Gate { id: 0xffffe }).unwrap();
    let result = engine
        .finish_provider_callback(KEY, 0, callback.token)
        .unwrap();
    assert_eq!(result.result, 0xabcdef01);
    assert_eq!(
        &engine.arena()[..EXIT_OFFSET + EXIT_SIZE],
        &before[..EXIT_OFFSET + EXIT_SIZE]
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, outer.token);
    engine.complete_core_windows_call(outer.token).unwrap();
}

#[test]
fn rejected_publication_is_neutral_and_an_outstanding_fault_cannot_be_overwritten() {
    let mut engine = fixture();
    let before = engine.arena().to_vec();
    let mut invalid = state(&engine);
    invalid.eflags = u32::MAX;
    assert!(publish(&mut engine, invalid, ExitReason::Budget).is_err());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.core_epoch, 0);
    let current = state(&engine);
    publish(
        &mut engine,
        current,
        ExitReason::MemoryFault {
            fault: MemoryFault {
                address: GuestAddress(0xdead0000),
                access: Access::Read,
                reason: FaultReason::Unmapped,
            },
            length: 4,
        },
    )
    .unwrap();
    assert!(engine.cpu_event.is_some());
    let before = engine.arena().to_vec();
    assert_eq!(
        publish(&mut engine, current, ExitReason::Budget),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn core_owners_are_instance_scoped_and_gate_identity_is_authoritative() {
    let mut engine = fixture();
    let other = fixture();
    assert!(engine.guard_core_owner(other.core_owner.unwrap()).is_err());
    let mut current = state(&engine);
    current.eip = 0x401000;
    publish(
        &mut engine,
        current,
        ExitReason::Gate {
            id: WindowsApi32::GetLastError.id(),
        },
    )
    .unwrap();
    let before = engine.arena().to_vec();
    assert!(engine.capture_core_windows_call().is_err());
    assert_eq!(engine.arena(), before);
    assert!(engine.pending_call.is_none());
}
