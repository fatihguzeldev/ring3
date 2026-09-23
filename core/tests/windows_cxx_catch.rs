#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const THROW: u32 = 0x7000_0490;
const HANDLER: u32 = 0x7000_0494;
const CODE: u32 = 0x0040_1000;
const CLEANUP: u32 = CODE + 0x20;
const CATCH: u32 = CODE + 0x40;
const CONTINUE: u32 = CODE + 0x60;
const INFO: u32 = 0x0040_2000;
const UNWIND: u32 = INFO + 0x100;
const TRY: u32 = INFO + 0x200;
const HANDLERS: u32 = INFO + 0x300;
const HANDLER_SLOT: u32 = INFO + 0x400;
const OBJECT: u32 = INFO + 0x500;
const THROW_INFO: u32 = INFO + 0x510;
const MARKER: u32 = INFO + 0x540;
const RECORD: u32 = 0x1000_ef00;
const STACK: u32 = RECORD - 0x200;
const INNER_RECORD: u32 = RECORD - 0x40;
const INNER_INFO: u32 = INFO + 0x600;
const INNER_UNWIND: u32 = INFO + 0x640;
const TYPE: u32 = INFO + 0x700;
const TYPE_ARRAY: u32 = INFO + 0x720;
const CATCHABLE: u32 = INFO + 0x740;
const INNER_HANDLER: u32 = CODE + 0x80;
const INNER_ACTION: u32 = CODE + 0x90;

fn put(process: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    process.memory.write(u64::from(address), &bytes).unwrap();
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn fixture() -> Process32 {
    fixture_variant(false)
}

fn fixture_variant(two_frame: bool) -> Process32 {
    let mut code = [0xcc; 0xc0];
    code[0] = 0xb8;
    code[1..5].copy_from_slice(&INFO.to_le_bytes());
    code[5..10].copy_from_slice(&[0xe9, 6, 0, 0, 0]);
    code[0x10..0x12].copy_from_slice(&[0xff, 0x25]);
    code[0x12..0x16].copy_from_slice(&HANDLER_SLOT.to_le_bytes());
    code[0x20..0x22].copy_from_slice(&[0xc7, 0x05]);
    code[0x22..0x26].copy_from_slice(&MARKER.to_le_bytes());
    code[0x26..0x2a].copy_from_slice(&1_u32.to_le_bytes());
    code[0x2a] = 0xc3;
    code[0x40..0x42].copy_from_slice(&[0x8b, 0x0d]);
    code[0x42..0x46].copy_from_slice(&MARKER.to_le_bytes());
    code[0x46] = 0xb8;
    code[0x47..0x4b].copy_from_slice(&CONTINUE.to_le_bytes());
    code[0x4b] = 0xc3;
    if two_frame {
        code[0x80] = 0xb8;
        code[0x81..0x85].copy_from_slice(&INNER_INFO.to_le_bytes());
        code[0x85..0x87].copy_from_slice(&[0xff, 0x25]);
        code[0x87..0x8b].copy_from_slice(&HANDLER_SLOT.to_le_bytes());
        code[0x90..0x92].copy_from_slice(&[0xc7, 0x05]);
        code[0x92..0x96].copy_from_slice(&MARKER.to_le_bytes());
        code[0x96..0x9a].copy_from_slice(&2_u32.to_le_bytes());
        code[0x9a] = 0xc3;
    }
    let mut process = Process32::load(
        &imported_executable::pe32(
            &code,
            "MSVCRT.dll",
            &["_CxxThrowException", "__CxxFrameHandler"],
        ),
        64,
    )
    .unwrap();
    put(&mut process, INFO, &[0x1993_0520, 4, UNWIND, 1, TRY, 0, 0]);
    put(&mut process, UNWIND + 2 * 8, &[1, CLEANUP]);
    put(&mut process, TRY, &[1, 2, 3, 1, HANDLERS]);
    put(&mut process, HANDLERS, &[0, 0, 0, CATCH]);
    put(&mut process, HANDLER_SLOT, &[HANDLER]);
    put(&mut process, THROW_INFO, &[0, 0, 0, 0]);
    put(&mut process, RECORD - 4, &[STACK + 12]);
    put(&mut process, RECORD, &[u32::MAX, CODE, 2]);
    put(&mut process, STACK, &[CONTINUE, OBJECT, THROW_INFO]);
    let fs_base = process.cpu.fs_base();
    put(&mut process, fs_base, &[RECORD]);
    process.cpu.set_register(Register32::Ebp, RECORD + 12);
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.eip = THROW;
    if two_frame {
        put(&mut process, INFO, &[0x1993_0520, 3, UNWIND, 1, TRY, 0, 0]);
        put(&mut process, TRY, &[1, 1, 2, 1, HANDLERS]);
        put(&mut process, RECORD + 8, &[1]);
        put(
            &mut process,
            INNER_INFO,
            &[0x1993_0520, 2, INNER_UNWIND, 0, 0, 0, 0],
        );
        put(&mut process, INNER_UNWIND + 8, &[u32::MAX, INNER_ACTION]);
        put(&mut process, INNER_RECORD, &[RECORD, INNER_HANDLER, 1]);
        put(&mut process, fs_base, &[INNER_RECORD]);
        put(&mut process, HANDLERS, &[0, TYPE, 0, CATCH]);
        put(&mut process, THROW_INFO, &[0, 0, 0, TYPE_ARRAY]);
        put(&mut process, TYPE_ARRAY, &[1, CATCHABLE]);
        put(&mut process, CATCHABLE, &[1, TYPE, 0, u32::MAX, 0, 4, 0]);
    }
    process
}

#[test]
fn guest_cleanup_runs_before_catch_and_resumes_at_saved_frame_stack() {
    let mut process = fixture();
    let mut reached = false;
    for _ in 0..32 {
        let result = process.run(1);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            reached = true;
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert!(reached);
    assert_eq!(process.cpu.register(Register32::Ecx), 1);
    assert_eq!(word(&process, MARKER), 1);
    assert_eq!(word(&process, RECORD + 8), 1);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
}

#[test]
fn malformed_metadata_typed_catch_and_object_destructor_stop_without_unwinding() {
    for case in 0..5 {
        let mut process = fixture();
        match case {
            0 => put(&mut process, INFO, &[0]),
            1 => put(&mut process, HANDLERS + 4, &[OBJECT]),
            2 => put(&mut process, THROW_INFO + 4, &[CLEANUP]),
            3 => put(&mut process, UNWIND + 2 * 8, &[2, CLEANUP]),
            _ => put(&mut process, RECORD - 4, &[STACK]),
        }
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: THROW }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, RECORD + 8), 2);
        assert_eq!(word(&process, MARKER), 0);
    }
}

#[test]
fn inner_cleanup_unlinks_its_frame_before_matching_simple_typed_catch() {
    let mut process = fixture_variant(true);
    let mut reached = false;
    for _ in 0..32 {
        let result = process.run(1);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            reached = true;
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert!(reached);
    assert_eq!(process.cpu.register(Register32::Ecx), 2);
    assert_eq!(word(&process, MARKER), 2);
    assert_eq!(word(&process, INNER_RECORD + 8), u32::MAX);
    assert_eq!(word(&process, process.cpu.fs_base()), RECORD);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
}

#[test]
fn unsafe_typed_or_inner_metadata_stops_before_guest_cleanup() {
    for case in 0..8 {
        let mut process = fixture_variant(true);
        match case {
            0 => put(&mut process, CATCHABLE + 4, &[OBJECT]),
            1 => put(&mut process, HANDLERS + 8, &[4]),
            2 => put(&mut process, CATCHABLE + 24, &[CLEANUP]),
            3 => put(&mut process, CATCHABLE + 20, &[8]),
            4 => put(&mut process, INNER_UNWIND + 8, &[0, INNER_ACTION]),
            5 => put(&mut process, INNER_INFO + 12, &[1]),
            6 => put(&mut process, THROW_INFO + 4, &[CLEANUP]),
            _ => process.cpu.set_register(Register32::Ebp, INNER_RECORD + 12),
        }
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: THROW }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, INNER_RECORD + 8), 1);
        assert_eq!(word(&process, process.cpu.fs_base()), INNER_RECORD);
        assert_eq!(word(&process, MARKER), 0);
    }
}
