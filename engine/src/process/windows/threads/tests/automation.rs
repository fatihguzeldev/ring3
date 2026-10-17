use super::{static_tls, timers};
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, MemoryError, PageRange, Permissions},
    process::{EngineInstance, HostError, ModuleRuntimeConfig32},
    windows::{
        automation::{Oleaut32Api32 as Api, variants::Variant32},
        thread::ThreadState32,
    },
};

const KEY: u64 = 83;
const METHOD: u32 = 0x401010;
const OBJECT: u32 = 0x403000;
const SOURCE: u32 = 0x403060;
const DESTINATION: u32 = 0x403080;

fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}
fn sealed_invoke() -> (EngineInstance, u32, u64, u32, u64, u32) {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_oleaut32_module(0x74000000).unwrap();
    let mut image = static_tls::image(0x400000, false, false);
    image[528..531].copy_from_slice(&[0xc2, 36, 0]);
    engine
        .register_guest_module("automation-exit.exe", &image)
        .unwrap();
    engine
        .start_module_process(
            "automation-exit.exe",
            None,
            ModuleRuntimeConfig32::default(),
        )
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.load_library_module("oleaut32.dll", None).unwrap();
    let caller = timers::call(&mut engine, Api::SysAllocStringLen.id(), &[0, 3]);
    let temporary = timers::call(&mut engine, Api::SysAllocStringLen.id(), &[0, 3]);
    let caller_identity = engine
        .windows_runtime_allocation_identity(caller - 4)
        .unwrap();
    let identity = engine
        .windows_runtime_allocation_identity(temporary - 4)
        .unwrap();
    write(&mut engine, OBJECT, &(OBJECT + 0x20).to_le_bytes());
    write(&mut engine, OBJECT + 0x20 + 24, &METHOD.to_le_bytes());
    write(
        &mut engine,
        SOURCE,
        &Variant32::new(9, &OBJECT.to_le_bytes()).0,
    );
    write(
        &mut engine,
        DESTINATION,
        &Variant32::new(8, &caller.to_le_bytes()).0,
    );
    let gates = engine.module_gates().unwrap();
    let gate = gates
        .iter()
        .find(|gate| gate.id == Api::VariantChangeTypeEx.id())
        .unwrap()
        .entry
        .0;
    let returned = gates
        .iter()
        .find(|gate| gate.id == super::super::INVOCATION_RETURN_ID)
        .unwrap()
        .entry
        .0;
    for (index, word) in [
        METHOD,
        3,
        gate,
        2,
        returned,
        2,
        gate,
        Api::VariantChangeTypeEx.id(),
        returned,
        super::super::INVOCATION_RETURN_ID,
    ]
    .into_iter()
    .enumerate()
    {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(3, 2).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate;
    state.registers[4] -= 64;
    for (index, word) in [0x401000, DESTINATION, SOURCE, 0x407, 0, 8]
        .into_iter()
        .enumerate()
    {
        write(
            &mut engine,
            state.registers[4] + index as u32 * 4,
            &word.to_le_bytes(),
        );
    }
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: Api::VariantChangeTypeEx.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(request.count, 9);
    assert_eq!(request.procedure.0, METHOD);
    let scratch = request.arguments[2];
    let callback = engine
        .begin_windows_callback(
            KEY,
            generation,
            returned,
            super::super::INVOCATION_RETURN_ID,
        )
        .unwrap();
    write(
        &mut engine,
        request.arguments[6],
        &Variant32::new(8, &temporary.to_le_bytes()).0,
    );
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = returned;
    state.registers[0] = 0;
    state.registers[4] = engine
        .callback
        .as_ref()
        .unwrap()
        .outer
        .frame
        .state()
        .registers[4];
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: super::super::INVOCATION_RETURN_ID,
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let teb = engine.module_teb_address().unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(teb), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert!(matches!(
        engine.finish_windows_callback(KEY, generation, callback.token),
        Err(HostError::Memory(_))
    ));
    assert!(
        engine.callback.is_none(),
        "accepted return must already be sealed"
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(
        engine.automation_thread_temporary_allocations(1).unwrap(),
        [(temporary - 4, identity)]
    );
    (
        engine,
        temporary,
        identity,
        caller,
        caller_identity,
        scratch,
    )
}

#[test]
fn sealed_invoke_temporary_survives_failed_thread_ram_retirement_then_retires_on_retry() {
    let (mut engine, temporary, identity, caller, caller_identity, scratch) = sealed_invoke();
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let arena = engine.arena().to_vec();
    let thread = engine.windows_thread_info(1).unwrap();
    let revision = engine.synchronization().unwrap().revision();
    let token = engine.pending_call.as_ref().unwrap().token;
    assert_eq!(
        engine.terminate_windows_thread(1, 41),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.windows_thread_info(1).unwrap(), thread);
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(
        engine.automation_thread_temporary_allocations(1).unwrap(),
        [(temporary - 4, identity)]
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    engine.terminate_windows_thread(1, 41).unwrap();
    assert_eq!(
        engine.windows_thread_info(1).unwrap().state,
        ThreadState32::Terminated
    );
    assert!(
        engine
            .windows_runtime_allocation_identity(temporary - 4)
            .is_err()
    );
    assert_eq!(
        engine
            .windows_runtime_allocation_identity(caller - 4)
            .unwrap(),
        caller_identity
    );
    assert!(
        engine
            .automation_process_temporary_allocations()
            .unwrap()
            .is_empty()
    );
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
}

#[test]
fn sealed_invoke_temporary_survives_failed_process_ram_retirement_then_retires_on_retry() {
    let (mut engine, temporary, identity, caller, caller_identity, scratch) = sealed_invoke();
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let arena = engine.arena().to_vec();
    let token = engine.pending_call.as_ref().unwrap().token;
    assert_eq!(
        engine.publish_process_exit(73),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.exit_code, None);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(
        engine.automation_process_temporary_allocations().unwrap(),
        [(temporary - 4, identity)]
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    engine.publish_process_exit(73).unwrap();
    assert_eq!(engine.exit_code, Some(73));
    assert!(
        engine
            .windows_runtime_allocation_identity(temporary - 4)
            .is_err()
    );
    assert_eq!(
        engine
            .windows_runtime_allocation_identity(caller - 4)
            .unwrap(),
        caller_identity
    );
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
}
