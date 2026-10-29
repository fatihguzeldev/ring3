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

#[cfg(feature = "external-cpu")]
#[test]
fn external_attach_accepts_retained_initializer_after_committed_mapping() {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    let main = crate::process::windows::process::test_image_for_startup();
    engine.register_guest_module("main.exe", &main).unwrap();
    engine
        .start_module_process("main.exe", None, Default::default())
        .unwrap();
    assert!(engine.external_cpu_module_ready());
    let mut dll = main;
    dll[0x96..0x98].copy_from_slice(&0x2103u16.to_le_bytes());
    dll[0xb4..0xb8].copy_from_slice(&0x600000u32.to_le_bytes());
    engine
        .register_guest_module("initializer.dll", &dll)
        .unwrap();
    let identity = engine.memory().unwrap().identity();
    let backing = engine.memory().unwrap().cpu_read_arrays().2.as_ptr();
    let crate::process::ModuleOperation32::Pending(initializer) =
        engine.load_library_module("initializer.dll", None).unwrap()
    else {
        panic!("missing initializer");
    };
    assert!(engine.has_active_module_operation());
    assert_eq!(engine.module_initializer().unwrap(), Some(initializer));
    assert_eq!(state(&engine).eip, initializer.entry);
    assert_eq!(engine.memory().unwrap().identity(), identity);
    assert_eq!(
        engine.memory().unwrap().cpu_read_arrays().2.as_ptr(),
        backing
    );
    assert!(engine.core_owner.is_none());
    assert!(engine.external_cpu_module_ready());
    engine.enable_core_execution().unwrap();
    engine.guard_core_entry().unwrap();
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_attach_rejects_pending_module_input() {
    let mut engine = EngineInstance::new(32, KEY).unwrap();
    assert!(engine.external_cpu_module_ready());
    engine.begin_module_input(1, "pending.dll").unwrap();
    assert!(!engine.external_cpu_module_ready());
    engine.abort_module_input().unwrap();
    assert!(engine.external_cpu_module_ready());
}

#[cfg(feature = "external-cpu")]
fn prepare_external_fast_gate(engine: &mut EngineInstance, id: u32) {
    let mut current = state(engine);
    current.eip = gate(engine, id);
    current.registers[4] -= 64;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(current.registers[4]),
            &0x401000u32.to_le_bytes(),
        )
        .unwrap();
    let banks = NumericBank::from_instance(engine)
        .unwrap()
        .architectural_state();
    let prepared = engine
        .prepare_core_exit(
            current,
            ExecutionExit {
                retired: 7,
                reason: ExitReason::Gate { id },
            },
            11,
            None,
            None,
            banks,
        )
        .unwrap();
    engine.commit_core_exit(prepared);
    engine.external_cpu_memory_pinned = true;
}

#[cfg(feature = "external-cpu")]
fn direct_result(engine: &EngineInstance) -> [u32; 4] {
    let packet = &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
    crate::abi::header::validate(packet, *b"R3IF", 32).unwrap();
    std::array::from_fn(|index| crate::abi::header::read_u32(packet, 16 + index * 4))
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_direct_gate_mask_admits_only_requested_known_ids_without_decline_mutation() {
    let mut engine = fixture();
    call(&mut engine, WindowsApi32::SetLastError, &[0xf1234567]);
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let pc = state(&engine).eip;
    let esp = state(&engine).registers[4];
    let before = engine.arena().to_vec();
    let epoch = engine.core_epoch;
    let token = engine.call_token;
    let version = engine.memory().unwrap().transaction_version();
    for mask in [0, 2, 0x100, 0x800, 0x801, u32::MAX] {
        assert!(
            !engine
                .try_direct_external_cpu_gate_mask(pc, esp, 0xabcdef01, mask)
                .unwrap()
        );
        assert_eq!(engine.arena(), before, "decline mask {mask:x}");
        assert_eq!(engine.core_epoch, epoch);
        assert_eq!(engine.call_token, token);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert!(engine.pending_call.is_none());
    }
    assert!(
        engine
            .try_direct_external_cpu_gate_mask(pc, esp, 0xabcdef01, 1)
            .unwrap()
    );
    assert_eq!(
        direct_result(&engine),
        [
            WindowsApi32::GetLastError.id(),
            0xf1234567,
            0x401000,
            esp + 4
        ]
    );
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_direct_gate_getter_uses_scalar_frame_without_publishing_cpu_state() {
    let mut engine = fixture();
    call(&mut engine, WindowsApi32::SetLastError, &[0xf1234567]);
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let pc = state(&engine).eip;
    let esp = state(&engine).registers[4];
    let mut canonical = state(&engine);
    canonical.eip = 0x401123;
    canonical.registers[0] = 0xabcdef01;
    encode_state_current(
        &canonical,
        &mut engine.arena.as_mut().get_mut()[..STATE_SIZE],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    let epoch = engine.core_epoch;
    let token = engine.call_token;
    let version = engine.memory().unwrap().transaction_version();
    assert!(
        engine
            .try_direct_external_cpu_gate(pc, esp, 0x778899aa)
            .unwrap()
    );
    assert_eq!(
        direct_result(&engine),
        [
            WindowsApi32::GetLastError.id(),
            0xf1234567,
            0x401000,
            esp + 4
        ]
    );
    assert_eq!(
        &engine.arena()[..TRANSFER_OFFSET],
        &before[..TRANSFER_OFFSET]
    );
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET + 32..],
        &before[TRANSFER_OFFSET + 32..]
    );
    assert_eq!(engine.core_epoch, epoch);
    assert_eq!(engine.call_token, token);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert!(engine.pending_call.is_none());
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_direct_gate_setter_and_tls_preserve_original_teb_and_void_result_semantics() {
    let mut engine = fixture();
    let index = engine.allocate_windows_tls().unwrap();
    engine.set_windows_tls(index, 0x55aa7788).unwrap();
    for (id, argument, eax, expected, error) in [
        (
            WindowsApi32::SetLastError.id(),
            0xf1234567,
            0xdecafbad,
            0xdecafbad,
            0xf1234567,
        ),
        (
            crate::windows::thread::ThreadApi32::TlsGetValue.id(),
            index,
            0x98765432,
            0x55aa7788,
            0,
        ),
    ] {
        prepare_external_fast_gate(&mut engine, id);
        engine.set_external_cpu_gate_mode(1).unwrap();
        let pc = state(&engine).eip;
        let esp = state(&engine).registers[4];
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(esp + 4), &argument.to_le_bytes())
            .unwrap();
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        let epoch = engine.core_epoch;
        let token = engine.call_token;
        assert!(engine.try_direct_external_cpu_gate(pc, esp, eax).unwrap());
        assert_eq!(direct_result(&engine), [id, expected, 0x401000, esp + 8]);
        assert_eq!(
            &engine.arena()[..TRANSFER_OFFSET],
            &before[..TRANSFER_OFFSET]
        );
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET + 32..],
            &before[TRANSFER_OFFSET + 32..]
        );
        let mut last_error = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(engine.module_teb_address().unwrap() + 0x34),
                &mut last_error,
            )
            .unwrap();
        assert_eq!(u32::from_le_bytes(last_error), error);
        assert_eq!(engine.memory().unwrap().transaction_version(), version + 1);
        assert_eq!(engine.core_epoch, epoch);
        assert_eq!(engine.call_token, token);
        assert!(engine.pending_call.is_none());
    }
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_direct_gate_decline_leaves_all_authoritative_state_untouched() {
    for (id, port, offset, guarded, captured) in [
        (WindowsApi32::GetLastError.id(), false, 0, false, false),
        (
            crate::windows::process::ProcessApi32::GetCurrentThreadId.id(),
            true,
            0,
            false,
            false,
        ),
        (WindowsApi32::GetLastError.id(), true, 1, false, false),
        (WindowsApi32::GetLastError.id(), true, 0, true, false),
        (WindowsApi32::GetLastError.id(), true, 0, false, true),
    ] {
        let mut engine = fixture();
        prepare_external_fast_gate(&mut engine, id);
        if port {
            engine.set_external_cpu_gate_mode(1).unwrap();
        }
        if guarded {
            let page = crate::memory::PageRange::new(
                GuestAddress(engine.module_teb_address().unwrap() & !4095),
                1,
            )
            .unwrap();
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(page, crate::memory::Permissions::READ_WRITE.with_guard())
                .unwrap();
        }
        if captured {
            engine.capture_core_windows_call().unwrap();
        }
        let pc = state(&engine).eip + offset;
        let esp = state(&engine).registers[4];
        let arena = engine.arena().to_vec();
        let epoch = engine.core_epoch;
        let token = engine.call_token;
        let version = engine.memory().unwrap().transaction_version();
        assert!(
            !engine
                .try_direct_external_cpu_gate(pc, esp, 0x12345678)
                .unwrap()
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.core_epoch, epoch);
        assert_eq!(engine.call_token, token);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.pending_call.is_some(), captured);
    }
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_inline_gate_publishes_zero_work_and_completes_original_getter_once() {
    let mut engine = fixture();
    call(&mut engine, WindowsApi32::SetLastError, &[0xf1234567]);
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let pc = state(&engine).eip;
    let stack = state(&engine).registers[4];
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Budget,
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let epoch = engine.core_epoch;
    let token = engine.call_token;
    let banks = crate::process::cpu::context::ExtendedCpuState::capture(engine.arena()).unwrap();
    assert!(
        engine
            .try_inline_external_cpu_gate_with(pc, |engine, token| {
                engine.complete_core_windows_call(token)
            })
            .unwrap()
    );
    assert_eq!(state(&engine).registers[0], 0xf1234567);
    assert_eq!(state(&engine).eip, 0x401000);
    assert_eq!(state(&engine).registers[4], stack + 4);
    assert_eq!(engine.core_epoch, epoch + 1);
    assert_eq!(engine.call_token, token + 1);
    assert_eq!(
        u32::from_le_bytes(
            engine.arena()[WORK_OFFSET + 16..WORK_OFFSET + 20]
                .try_into()
                .unwrap()
        ),
        0
    );
    assert_eq!(
        decode_exit(&engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]).unwrap(),
        ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode
        }
    );
    assert!(banks.matches(engine.arena()));
    assert!(engine.pending_call.is_none());
    let record = crate::abi::call_frame::decode_call_frame(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112],
    )
    .unwrap();
    assert_eq!(record.id, WindowsApi32::GetLastError.id());
    assert_eq!(record.token, token + 1);
    let arena = engine.arena().to_vec();
    assert!(
        !engine
            .try_inline_external_cpu_gate_with(pc, |_, _| panic!("completed gate replayed"))
            .unwrap()
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.call_token, token + 1);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_inline_gate_declines_unowned_unsupported_and_stale_inputs_without_mutation() {
    for (id, port, offset, guarded) in [
        (WindowsApi32::GetLastError.id(), false, 0, false),
        (
            crate::windows::process::ProcessApi32::GetCurrentThreadId.id(),
            true,
            0,
            false,
        ),
        (WindowsApi32::GetLastError.id(), true, 1, false),
        (WindowsApi32::GetLastError.id(), true, 0, true),
    ] {
        let mut engine = fixture();
        prepare_external_fast_gate(&mut engine, id);
        if port {
            engine.set_external_cpu_gate_mode(1).unwrap();
        }
        let guarded_page = guarded.then(|| {
            crate::memory::PageRange::new(
                GuestAddress(engine.module_teb_address().unwrap() & !4095),
                1,
            )
            .unwrap()
        });
        if let Some(page) = guarded_page {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(page, crate::memory::Permissions::READ_WRITE.with_guard())
                .unwrap();
        }
        let pc = state(&engine).eip + offset;
        let arena = engine.arena().to_vec();
        let epoch = engine.core_epoch;
        let token = engine.call_token;
        assert!(
            !engine
                .try_inline_external_cpu_gate_with(pc, |_, _| panic!("decline captured a call"))
                .unwrap()
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.core_epoch, epoch);
        assert_eq!(engine.call_token, token);
        assert!(engine.pending_call.is_none());
        if let Some(page) = guarded_page {
            assert_eq!(
                engine
                    .memory()
                    .unwrap()
                    .permissions_at(GuestAddress((page.first * 4096) as u32))
                    .unwrap(),
                crate::memory::Permissions::READ_WRITE.with_guard()
            );
        }
    }
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_inline_gate_post_capture_busy_is_terminal_and_preserves_original_capture() {
    let mut engine = fixture();
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let pc = state(&engine).eip;
    let epoch = engine.core_epoch;
    let token = engine.call_token;
    assert_eq!(
        engine.try_inline_external_cpu_gate_with(pc, |engine, captured| {
            assert_eq!(engine.pending_call.as_ref().unwrap().token, captured);
            engine.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112].fill(0xcc);
            Err(HostError::Call(CallError::Busy))
        }),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.core_epoch, epoch + 1);
    assert_eq!(engine.call_token, token + 1);
    let record = crate::abi::call_frame::decode_call_frame(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112],
    )
    .unwrap();
    assert_eq!(record.id, WindowsApi32::GetLastError.id());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, record.token);
    let arena = engine.arena().to_vec();
    assert!(
        !engine
            .try_inline_external_cpu_gate_with(pc, |_, _| panic!("captured Busy replayed"))
            .unwrap()
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.call_token, token + 1);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_fast_gate_completes_getter_and_preserves_work_and_numeric_banks() {
    let mut engine = fixture();
    call(&mut engine, WindowsApi32::SetLastError, &[0xf1234567]);
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    let stack = state(&engine).registers[4];
    let work = engine.arena()[WORK_OFFSET..WORK_OFFSET + WORK_SIZE].to_vec();
    let banks = engine.arena()[X87_OFFSET..YMM_OFFSET + YMM_SIZE].to_vec();
    assert!(engine.try_complete_external_cpu_gate().unwrap());
    assert_eq!(state(&engine).registers[0], 0xf1234567);
    assert_eq!(state(&engine).eip, 0x401000);
    assert_eq!(state(&engine).registers[4], stack + 4);
    assert_eq!(&engine.arena()[WORK_OFFSET..WORK_OFFSET + WORK_SIZE], work);
    assert_eq!(&engine.arena()[X87_OFFSET..YMM_OFFSET + YMM_SIZE], banks);
    assert!(engine.pending_call.is_none());
    let record = crate::abi::call_frame::decode_call_frame(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112],
    )
    .unwrap();
    assert_eq!(record.id, WindowsApi32::GetLastError.id());
    assert_eq!(record.return_pc, 0x401000);
    assert!(engine.complete_core_windows_call(record.token).is_err());
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_fast_gate_declines_mismatched_or_uncommitted_gates_without_mutation() {
    let mut engine = fixture();
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    let current = state(&engine);
    publish(
        &mut engine,
        current,
        ExitReason::Gate {
            id: WindowsApi32::SetLastError.id(),
        },
    )
    .unwrap();
    let arena = engine.arena().to_vec();
    let token = engine.call_token;
    assert!(!engine.try_complete_external_cpu_gate().unwrap());
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.call_token, token);
    assert!(engine.pending_call.is_none());
    let mut current = state(&engine);
    current.eip = gate(&engine, WindowsApi32::GetLastError.id());
    publish(&mut engine, current, ExitReason::NeedCode).unwrap();
    let arena = engine.arena().to_vec();
    assert!(!engine.try_complete_external_cpu_gate().unwrap());
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.call_token, token);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_fast_gate_busy_retains_the_original_capture_for_one_later_completion() {
    let mut engine = fixture();
    prepare_external_fast_gate(&mut engine, WindowsApi32::GetLastError.id());
    let result = engine.try_complete_external_cpu_gate_with(|engine, token| {
        assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
        engine.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112].fill(0xcc);
        Err(HostError::Call(CallError::Busy))
    });
    assert_eq!(result, Err(HostError::Call(CallError::Busy)));
    let record = crate::abi::call_frame::decode_call_frame(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112],
    )
    .unwrap();
    assert_eq!(engine.pending_call.as_ref().unwrap().token, record.token);
    assert_eq!(record.id, WindowsApi32::GetLastError.id());
    assert_eq!(
        engine.try_complete_external_cpu_gate(),
        Err(HostError::InvalidRequest)
    );
    engine.complete_core_windows_call(record.token).unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(state(&engine).eip, 0x401000);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_fast_gate_set_last_error_keeps_the_real_teb_write_and_return_state() {
    let mut engine = fixture();
    prepare_external_fast_gate(&mut engine, WindowsApi32::SetLastError.id());
    let stack = state(&engine).registers[4];
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(stack + 4), &0xfedc1234u32.to_le_bytes())
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let work = engine.arena()[WORK_OFFSET..WORK_OFFSET + WORK_SIZE].to_vec();
    assert!(engine.try_complete_external_cpu_gate().unwrap());
    let teb = engine.module_teb_address().unwrap();
    let mut word = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(teb + 0x34), &mut word)
        .unwrap();
    assert_eq!(u32::from_le_bytes(word), 0xfedc1234);
    assert!(engine.memory().unwrap().transaction_version() > version);
    assert_eq!(state(&engine).registers[0], 0);
    assert_eq!(state(&engine).registers[4], stack + 8);
    assert_eq!(state(&engine).eip, 0x401000);
    assert_eq!(&engine.arena()[WORK_OFFSET..WORK_OFFSET + WORK_SIZE], work);
    assert!(engine.pending_call.is_none());
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_fast_gate_completes_another_registered_process_api() {
    let mut engine = fixture();
    prepare_external_fast_gate(
        &mut engine,
        crate::windows::process::ProcessApi32::GetCurrentThreadId.id(),
    );
    let thread = engine.current_windows_thread_id().unwrap();
    let stack = state(&engine).registers[4];
    assert!(engine.try_complete_external_cpu_gate().unwrap());
    assert_eq!(state(&engine).registers[0], thread);
    assert_eq!(state(&engine).registers[4], stack + 4);
    assert_eq!(state(&engine).eip, 0x401000);
    assert!(engine.pending_call.is_none());
    let record = crate::abi::call_frame::decode_call_frame(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112],
    )
    .unwrap();
    assert_eq!(
        record.id,
        crate::windows::process::ProcessApi32::GetCurrentThreadId.id()
    );
}

#[cfg(feature = "external-cpu")]
fn external_gate_bytes(engine: &EngineInstance, pc: u32) -> [u8; 2] {
    let mut bytes = [0; 2];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(pc), &mut bytes)
        .unwrap();
    bytes
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_port_gate_mode_patches_rx_and_lazily_owns_new_module_gates() {
    use crate::memory::{PageRange, Permissions};
    let mut engine = fixture();
    engine.external_cpu_memory_pinned = true;
    let id = WindowsApi32::GetLastError.id();
    let pc = gate(&engine, id);
    let rights = engine
        .memory()
        .unwrap()
        .permissions_at(GuestAddress(pc))
        .unwrap();
    assert_eq!(external_gate_bytes(&engine, pc), [0x0f, 0x0b]);
    let before = engine.memory().unwrap().code_validation_version();
    assert!(engine.set_external_cpu_gate_mode(2).is_err());
    assert_eq!(engine.memory().unwrap().code_validation_version(), before);
    let (mode, count) = engine.set_external_cpu_gate_mode(1).unwrap();
    assert_eq!(mode, 1);
    assert!(count > 0);
    assert_eq!(external_gate_bytes(&engine, pc), [0xe6, 0xe1]);
    assert_eq!(engine.external_cpu_gate_match(pc), Some((id, true)));
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(pc))
            .unwrap(),
        rights
    );
    assert!(engine.memory().unwrap().code_validation_version() > before);
    let unregistered = 0x690000;
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(unregistered), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(unregistered), &[0xe6, 0xe1])
        .unwrap();
    assert_eq!(engine.external_cpu_lookup_gate(unregistered).unwrap(), None);
    engine.register_user32_module(0x71000000).unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    let new_gate = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id >> 16 == 2)
        .unwrap();
    assert_eq!(external_gate_bytes(&engine, new_gate.entry.0), [0x0f, 0x0b]);
    assert_eq!(
        engine.external_cpu_lookup_gate(new_gate.entry.0).unwrap(),
        Some((new_gate.id, false)),
        "the first real UD2 retains its provenance while the next entry is patched"
    );
    assert_eq!(
        engine.external_cpu_gate_match(new_gate.entry.0),
        Some((new_gate.id, true))
    );
    let before_restore = engine.memory().unwrap().code_validation_version();
    assert_eq!(engine.set_external_cpu_gate_mode(0).unwrap(), (0, 0));
    assert_eq!(external_gate_bytes(&engine, pc), [0x0f, 0x0b]);
    assert_eq!(external_gate_bytes(&engine, new_gate.entry.0), [0x0f, 0x0b]);
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(pc))
            .unwrap(),
        rights
    );
    assert!(engine.memory().unwrap().code_validation_version() > before_restore);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_port_gate_restore_rejects_modified_bytes_without_partial_restore() {
    use crate::memory::{PageRange, Permissions};
    let mut engine = fixture();
    engine.external_cpu_memory_pinned = true;
    let pc = gate(&engine, WindowsApi32::GetLastError.id());
    let other = gate(&engine, WindowsApi32::SetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !4095), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::ALL)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(pc), &[0x90, 0x90])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ_EXECUTE)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert!(engine.set_external_cpu_gate_mode(0).is_err());
    assert_eq!(external_gate_bytes(&engine, pc), [0x90, 0x90]);
    assert_eq!(external_gate_bytes(&engine, other), [0xe6, 0xe1]);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.external_cpu_memory_pinned);
}

#[cfg(feature = "external-cpu")]
#[test]
fn external_port_gate_restore_rejects_same_va_remap_even_with_matching_port_bytes() {
    use crate::memory::{PageRange, Permissions};
    let mut engine = fixture();
    engine.external_cpu_memory_pinned = true;
    let pc = gate(&engine, WindowsApi32::GetLastError.id());
    engine.set_external_cpu_gate_mode(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !4095), 1).unwrap();
    engine.memory.as_mut().unwrap().unmap(page).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(page, Permissions::ALL)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(pc), &[0xe6, 0xe1])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ_EXECUTE)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert_eq!(engine.external_cpu_gate_match(pc), None);
    assert!(engine.set_external_cpu_gate_mode(0).is_err());
    assert_eq!(external_gate_bytes(&engine, pc), [0xe6, 0xe1]);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.external_cpu_memory_pinned);
}
