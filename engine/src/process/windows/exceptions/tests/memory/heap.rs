use super::*;

fn last_error(engine: &mut EngineInstance) {
    let thread = engine
        .current_windows_thread()
        .unwrap()
        .with_last_error(0x55);
    engine.publish_windows_thread(thread).unwrap();
}

#[test]
fn inherited_heap_generate_flag_raises_no_memory_through_real_captured_call() {
    let mut engine = engine();
    let heap = call(&mut engine, MemoryApi32::HeapCreate, &[4, 0, 65536]).unwrap();
    assert_ne!(heap, 0);
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    last_error(&mut engine);
    call(&mut engine, MemoryApi32::HeapAlloc, &[heap, 0, 65537]).unwrap();
    let dispatch = engine.pending_windows_exception().unwrap();
    assert_eq!(dispatch.code, 0xc0000017);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 0x55);
    returned(&mut engine, dispatch, u32::MAX, false);
    assert!(
        engine
            .finish_windows_exception_handler(KEY, dispatch.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
}

#[test]
fn explicit_heap_generate_flag_distinguishes_invalid_reallocation_from_plain_null_failure() {
    for flags in [0, 4] {
        let mut engine = engine();
        let heap = call(&mut engine, MemoryApi32::HeapCreate, &[0, 0, 65536]).unwrap();
        engine
            .exceptions
            .handlers
            .add(false, true, 0x401000)
            .unwrap();
        last_error(&mut engine);
        call(
            &mut engine,
            MemoryApi32::HeapReAlloc,
            &[heap, flags, 0xdead, 8],
        )
        .unwrap();
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 0x55);
        if flags == 4 {
            assert_eq!(engine.pending_windows_exception().unwrap().code, 0xc0000005);
        } else {
            assert!(engine.pending_windows_exception().is_none());
            assert_eq!(
                decode_state(&engine.arena()[..STATE_SIZE])
                    .unwrap()
                    .registers[0],
                0
            );
        }
    }
}

#[test]
fn failed_heap_exception_preflight_keeps_original_captured_call_and_ram() {
    let mut engine = engine();
    let heap = call(&mut engine, MemoryApi32::HeapCreate, &[4, 0, 65536]).unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x600000)
        .unwrap();
    last_error(&mut engine);
    let (generation, token) = capture(&mut engine, MemoryApi32::HeapAlloc, &[heap, 0, 65537]);
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    assert!(
        engine
            .complete_windows_call(KEY, generation, token)
            .is_err()
    );
    assert!(engine.pending_call.is_some());
    assert!(engine.pending_windows_exception().is_none());
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 0x55);
}

#[test]
fn enabled_heap_corruption_is_terminal_and_bypasses_registered_exception_handlers() {
    let mut engine = engine();
    let heap = call(&mut engine, MemoryApi32::GetProcessHeap, &[]).unwrap();
    let pointer = call(&mut engine, MemoryApi32::HeapAlloc, &[heap, 0, 16]).unwrap();
    call(&mut engine, MemoryApi32::HeapSetInformation, &[0, 1, 0, 0]).unwrap();
    call(&mut engine, MemoryApi32::HeapFree, &[heap, 0, pointer]).unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    last_error(&mut engine);
    call(&mut engine, MemoryApi32::HeapFree, &[heap, 0, pointer]).unwrap();
    let dispatch = engine.pending_windows_exception().unwrap();
    assert_eq!(
        dispatch.phase,
        crate::process::windows::exceptions::ExceptionPhase32::Unhandled
    );
    assert_eq!(dispatch.procedure, 0);
    assert_eq!(dispatch.code, 0xc0000374);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 0x55);
    assert!(engine.pending_call.is_none());
    engine
        .terminate_windows_exception(KEY, dispatch.id)
        .unwrap();
    assert_eq!(engine.exit_code, Some(0xc0000374));
}
