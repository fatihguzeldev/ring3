use super::*;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::exceptions::ExceptionRecord32,
};
#[path = "tests/memory/mod.rs"]
mod memory;
fn engine() -> EngineInstance {
    let mut image = vec![0; 1024];
    image[..2].copy_from_slice(b"MZ");
    let put16 = |bytes: &mut [u8], at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes())
    };
    let put32 = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes())
    };
    put32(&mut image, 0x3c, 0x80);
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14c),
        (0x86, 1),
        (0x94, 224),
        (0x96, 0x103),
        (0x98, 0x10b),
    ] {
        put16(&mut image, at, value);
    }
    for (at, value) in [
        (0xa8, 0x1000),
        (0xb4, 0x400000),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x2000),
        (0xd4, 512),
        (0xf4, 16),
    ] {
        put32(&mut image, at, value);
    }
    put16(&mut image, 0xdc, 3);
    let section = 0x178;
    image[section..section + 8].copy_from_slice(b".text\0\0\0");
    for (at, value) in [
        (section + 8, 512),
        (section + 12, 4096),
        (section + 16, 512),
        (section + 20, 512),
        (section + 36, 0x60000020),
    ] {
        put32(&mut image, at, value);
    }
    image[512] = 0xc3;
    let mut engine = EngineInstance::new(128, 77).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module("exceptions.exe", &image)
        .unwrap();
    engine
        .start_module_process(
            "exceptions.exe",
            None,
            ModuleRuntimeConfig32 {
                stack_base: 0x7f000000,
                stack_pages: 4,
                runtime_base: 0x7e000000,
            },
        )
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine
}
fn returned(engine: &mut EngineInstance, dispatch: ExceptionDispatch32, value: u32, cdecl: bool) {
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = dispatch.return_pc;
    state.registers[0] = value;
    state.registers[4] =
        engine.exceptions.deliveries.last().unwrap().stack - if cdecl { 16 } else { 0 };
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: dispatch.return_id,
            },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}
#[test]
fn vectored_handler_resumes_the_guest_edited_context_and_cannot_replay_return() {
    let mut engine = engine();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    let original = engine.windows_context_state().unwrap();
    let dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000094, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    let mut context = [0; 716];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(dispatch.context), &mut context)
        .unwrap();
    context[184..188].copy_from_slice(&0x401001u32.to_le_bytes());
    context[176..180].copy_from_slice(&42u32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(dispatch.context), &context)
        .unwrap();
    returned(&mut engine, dispatch, u32::MAX, false);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, dispatch.id)
            .unwrap(),
        None
    );
    let resumed = engine.windows_context_state().unwrap();
    assert_eq!(resumed.state.eip, 0x401001);
    assert_eq!(resumed.state.registers[0], 42);
    assert_eq!(resumed.x87, original.x87);
    assert_eq!(resumed.simd, original.simd);
    assert!(
        engine
            .finish_windows_exception_handler(77, dispatch.id)
            .is_err()
    );
}
#[test]
fn structured_handlers_receive_registration_frames_and_use_cdecl_return_stack() {
    let mut engine = engine();
    let original = engine.windows_context_state().unwrap();
    let first = original.state.registers[4] - 64;
    let second = first + 16;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(0x7e000000), &first.to_le_bytes()),
            (
                GuestAddress(first),
                &[second.to_le_bytes(), 0x401000u32.to_le_bytes()].concat(),
            ),
            (
                GuestAddress(second),
                &[u32::MAX.to_le_bytes(), 0x401001u32.to_le_bytes()].concat(),
            ),
        ])
        .unwrap();
    // The active stack frame must lie below the exception registration records.
    let mut original = original;
    original.state.registers[4] = first - 16;
    engine.publish_windows_context(&original).unwrap();
    let first_dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0000001, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(first_dispatch.phase, ExceptionPhase32::Structured);
    returned(&mut engine, first_dispatch, 1, true);
    let second_dispatch = engine
        .finish_windows_exception_handler(77, first_dispatch.id)
        .unwrap()
        .unwrap();
    assert_eq!(second_dispatch.procedure, 0x401001);
    returned(&mut engine, second_dispatch, 0, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, second_dispatch.id)
            .unwrap(),
        None
    );
}
#[test]
fn malformed_chain_does_not_publish_scratch_or_consume_original_cpu_state() {
    let mut engine = engine();
    let original = engine.windows_context_state().unwrap();
    let frame = original.state.registers[4] - 16;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(0x7e000000), &frame.to_le_bytes()),
            (
                GuestAddress(frame),
                &[frame.to_le_bytes(), 0x401000u32.to_le_bytes()].concat(),
            ),
        ])
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    assert!(
        engine
            .begin_exception_delivery(
                ExceptionRecord32::new(1, 0, original.state.eip, &[]).unwrap(),
                original,
                PendingOwner::Replacement(0),
                None,
                None,
                None
            )
            .is_err()
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert_eq!(engine.arena(), arena);
    assert!(engine.pending_windows_exception().is_none());
}

#[path = "tests/review.rs"]
mod review;

#[path = "tests/context.rs"]
mod context;
