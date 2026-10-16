use super::*;
use crate::{
    process::ExecutionConfig32,
    windows::{CallingConvention32, thread::ThreadApi32},
};

const KEY: u64 = 91;

fn install(engine: &mut EngineInstance, pc: u32) -> u64 {
    let request = engine.prepare_cache_entry(KEY, pc).unwrap();
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    request.unit_id
}

#[test]
fn cached_callback_returns_through_a_separate_exact_runtime_gate_unit() {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .start_module_process("main.exe", None, Default::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x90, 0xc3])
        .unwrap();
    engine
        .configure_execution(ExecutionConfig32 {
            units: 16,
            wasm_bytes: 1024 * 1024,
            promotion_hits: u32::MAX,
        })
        .unwrap();
    let gates = engine.module_gates().unwrap();
    let outer = gates
        .iter()
        .find(|gate| gate.id == ThreadApi32::TlsAlloc.id())
        .unwrap();
    let return_gate = gates
        .iter()
        .find(|gate| gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID)
        .unwrap();
    let (outer_pc, outer_gate) = (outer.entry.0, outer.id);
    let (return_pc, return_id) = (return_gate.entry.0, return_gate.id);
    let outer_id = install(&mut engine, outer_pc);
    let home = install(&mut engine, 0x1000);
    engine.write32(0x8100, 0x2000).unwrap();
    super::nesting::stop(&mut engine, outer_pc, outer_gate, 0x8100, 0);
    let call = engine
        .capture_resident_call(KEY, outer_id, CallingConvention32::Cdecl, 0)
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.begin_resident_callback(
            KEY,
            outer_id,
            home,
            call.token,
            0x1000,
            return_pc,
            return_id ^ 1,
            &[],
        ),
        Err(HostError::Call(CallError::InvalidRequest))
    );
    assert_eq!(engine.arena(), before);
    let callback = engine
        .begin_resident_callback(
            KEY,
            outer_id,
            home,
            call.token,
            0x1000,
            return_pc,
            return_id,
            &[],
        )
        .unwrap();
    engine
        .authorize_resident_callback(KEY, home, callback.token)
        .unwrap();
    let tail = install(&mut engine, return_pc);
    let mut state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = return_pc;
    state.registers[4] = 0x8100;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 2,
            reason: ExitReason::NeedCode,
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .select_resident_callback_unit(KEY, home, callback.token, tail)
        .unwrap();
    super::nesting::stop(&mut engine, return_pc, return_id ^ 1, 0x8100, 42);
    assert_eq!(
        engine.finish_resident_callback(KEY, home, callback.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    super::nesting::stop(&mut engine, return_pc, return_id, 0x8100, 42);
    assert_eq!(
        engine.finish_resident_callback(KEY, tail, callback.token),
        Err(HostError::Call(CallError::InvalidToken))
    );
    let returned = engine
        .finish_resident_callback(KEY, home, callback.token)
        .unwrap();
    assert_eq!(returned.result, 42);
    assert_eq!(returned.callback_unit_id, home);
    assert_eq!(returned.outer_unit_id, outer_id);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, call.token);
}
