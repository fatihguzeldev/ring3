use super::*;
use crate::process::HostError;
use crate::windows::exceptions::context::{CONTEXT_ALL32, Win32Context32};

fn captured_call(engine: &mut EngineInstance, id: u32, args: &[u32]) {
    captured_call_using(engine, id, args, None).unwrap();
}

fn captured_call_using(
    engine: &mut EngineInstance,
    id: u32,
    args: &[u32],
    convention: Option<crate::windows::CallingConvention32>,
) -> Result<(), HostError> {
    let gate = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    for (index, word) in [gate, 2, gate, id].into_iter().enumerate() {
        engine.arena.as_mut().get_mut()
            [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let mut context = engine.windows_context_state().unwrap();
    let stack = 0x7f003000;
    let words: Vec<u8> = std::iter::once(0x401010u32)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(stack), &words)
        .unwrap();
    context.state.eip = gate;
    context.state.registers[4] = stack;
    engine.publish_windows_context(&context).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = match convention {
        Some(convention) => {
            engine.capture_call(77, engine.generation, convention, args.len() as u32)
        }
        None => engine.capture_windows_call(77, engine.generation),
    }
    .unwrap()
    .token;
    engine.complete_windows_call(77, engine.generation, token)
}

#[test]
fn rtlrestorecontext_uses_cdecl_metadata_and_rejects_a_stdcall_captured_frame() {
    use crate::windows::CallingConvention32;
    let api = ExceptionApi32::RtlRestoreContext;
    assert_eq!(api.convention(), CallingConvention32::Cdecl);
    assert_eq!(
        crate::process::windows::calls::windows_gate_info(api.id())
            .unwrap()
            .convention,
        CallingConvention32::Cdecl
    );
    for convention in [None, Some(CallingConvention32::Stdcall)] {
        let mut engine = engine();
        engine.load_library_module("ntdll.dll", None).unwrap();
        let mut requested = engine.windows_context_state().unwrap();
        requested.state.eip = 0x401010;
        requested.state.registers[0] = 0x71324680;
        requested.state.registers[4] = 0x7f003100;
        let context = Win32Context32::capture(CONTEXT_ALL32, &requested).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x7f003800), context.bytes())
            .unwrap();
        let result = captured_call_using(&mut engine, api.id(), &[0x7f003800, 0], convention);
        if convention.is_none() {
            result.unwrap();
            assert_eq!(engine.windows_context_state().unwrap(), requested);
            assert!(engine.pending_call.is_none());
        } else {
            assert!(matches!(
                result,
                Err(HostError::Call(crate::process::CallError::InvalidRequest))
            ));
            assert!(engine.pending_call.is_some());
        }
    }
}

#[test]
fn ntraise_second_chance_preserves_requested_record_and_context_without_repeating_handlers() {
    for first_chance in [false, true] {
        let mut engine = engine();
        engine.load_library_module("ntdll.dll", None).unwrap();
        let mut requested = engine.windows_context_state().unwrap();
        requested.state.eip = 0x401010;
        requested.state.registers[0] = 0x12345678;
        requested.state.registers[4] = 0x7f003100;
        let context = Win32Context32::capture(CONTEXT_ALL32, &requested).unwrap();
        let record = ExceptionRecord32::new(0xe0411234, 0, 0x401020, &[7, 19]).unwrap();
        let registration = 0x7f003f00u32;
        engine
            .memory
            .as_mut()
            .unwrap()
            .write_spans(&[
                (GuestAddress(0x7f003d00), &record.bytes()),
                (GuestAddress(0x7f003800), context.bytes()),
                (GuestAddress(0x7e000000), &registration.to_le_bytes()),
                (
                    GuestAddress(registration),
                    &[u32::MAX.to_le_bytes(), 0x401001u32.to_le_bytes()].concat(),
                ),
            ])
            .unwrap();
        engine
            .exceptions
            .handlers
            .add(false, true, 0x401000)
            .unwrap();
        engine.exceptions.handlers.set_filter(0x401002);
        captured_call(
            &mut engine,
            ExceptionApi32::NtRaiseException.id(),
            &[0x7f003d00, 0x7f003800, u32::from(first_chance)],
        );
        assert!(engine.pending_call.is_none());
        let dispatch = engine.pending_windows_exception().unwrap();
        let delivery = engine.exceptions.deliveries.last().unwrap();
        assert_eq!(delivery.record, record);
        assert_eq!(delivery.original, requested);
        let mut record_bytes = [0; 80];
        let mut context_bytes = [0; 716];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(dispatch.record), &mut record_bytes)
            .unwrap();
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(dispatch.context), &mut context_bytes)
            .unwrap();
        assert_eq!(record_bytes, record.bytes());
        assert_eq!(&context_bytes, context.bytes());
        if first_chance {
            assert_eq!(dispatch.phase, ExceptionPhase32::Vectored);
            assert_eq!(dispatch.procedure, 0x401000);
            returned(&mut engine, dispatch, 0, false);
            let structured = engine
                .finish_windows_exception_handler(77, dispatch.id)
                .unwrap()
                .unwrap();
            assert_eq!(structured.phase, ExceptionPhase32::Structured);
            assert_eq!(structured.procedure, 0x401001);
            returned(&mut engine, structured, 1, true);
            let filter = engine
                .finish_windows_exception_handler(77, dispatch.id)
                .unwrap()
                .unwrap();
            assert_eq!(filter.phase, ExceptionPhase32::Filter);
            assert_eq!(filter.procedure, 0x401002);
            returned(&mut engine, filter, 0, false);
            let unhandled = engine
                .finish_windows_exception_handler(77, dispatch.id)
                .unwrap()
                .unwrap();
            assert_eq!(unhandled.phase, ExceptionPhase32::Unhandled);
        } else {
            assert_eq!(dispatch.phase, ExceptionPhase32::Unhandled);
            assert_eq!(dispatch.procedure, 0);
            assert!(delivery.handlers.is_empty());
            assert!(delivery.visited.is_empty());
        }
    }
}

#[test]
fn terminal_unhandled_without_pending_call_finalizes_files_and_rejects_wrong_or_replayed_identity()
{
    use crate::{files::OverlayKind32, windows::files::FileApi32};
    let mut engine = engine();
    engine
        .configure_files(crate::process::FileConfig32::default())
        .unwrap();
    let epoch = engine.files_epoch().unwrap();
    let path: Vec<u8> = r"C:\Temp\unhandled.tmp"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x7f002000), &path)
        .unwrap();
    captured_call(
        &mut engine,
        FileApi32::CreateFileW.id(),
        &[0x7f002000, 0xc0010000, 7, 0, 4, 0x04000080, 0],
    );
    assert_ne!(
        engine.windows_context_state().unwrap().state.registers[0],
        u32::MAX
    );
    assert!(engine.pending_call.is_none());
    let original = engine.windows_context_state().unwrap();
    let record = ExceptionRecord32::new(0xe041feed, 0, original.state.eip, &[3]).unwrap();
    let dispatch = engine
        .begin_exception_delivery_with_page_change(
            record,
            original,
            PendingOwner::Replacement(engine.generation),
            None,
            None,
            ExceptionInitial32 {
                last_chance: true,
                ..ExceptionInitial32::default()
            },
        )
        .unwrap();
    assert_eq!(dispatch.phase, ExceptionPhase32::Unhandled);
    assert!(engine.pending_call.is_none());
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert!(
        engine
            .terminate_windows_exception(77, dispatch.id + 1)
            .is_err()
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.pending_windows_exception(), Some(dispatch));
    assert_eq!(engine.exit_code, None);
    engine.terminate_windows_exception(77, dispatch.id).unwrap();
    assert_eq!(engine.exit_code, Some(record.code));
    let primary = engine.threads.info(1).unwrap();
    assert_eq!(
        primary.state,
        crate::windows::thread::ThreadState32::Terminated
    );
    assert_eq!(primary.exit_code, record.code);
    assert_eq!(
        decode_exit(&engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .unwrap()
            .reason,
        ExitReason::ProcessExited { code: record.code }
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_exception().is_none());
    let mut deleted = false;
    for index in 0..16 {
        let Some(entry) = engine.file_overlay_entry(epoch, index).unwrap() else {
            break;
        };
        if entry.path == r"C:\Temp\unhandled.tmp".encode_utf16().collect::<Vec<_>>() {
            assert_eq!(entry.kind, OverlayKind32::Deleted);
            deleted = true;
        }
    }
    assert!(deleted);
    assert!(engine.terminate_windows_exception(77, dispatch.id).is_err());
}
