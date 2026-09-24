use ring3_core::execution::{
    Access, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

use super::imported_executable;

pub const THROW: u32 = 0x7000_0490;
pub const HANDLER: u32 = 0x7000_0494;
pub const CODE: u32 = 0x0040_1000;
pub const CLEANUP: u32 = CODE + 0x20;
pub const CATCH: u32 = CODE + 0x40;
pub const CONTINUE: u32 = CODE + 0x60;
pub const INFO: u32 = 0x0040_2000;
pub const UNWIND: u32 = INFO + 0x100;
pub const TRY: u32 = INFO + 0x200;
pub const HANDLERS: u32 = INFO + 0x300;
pub const HANDLER_SLOT: u32 = INFO + 0x400;
pub const OBJECT: u32 = INFO + 0x500;
pub const THROW_INFO: u32 = INFO + 0x510;
pub const MARKER: u32 = INFO + 0x540;
pub const RECORD: u32 = 0x1000_ef00;
pub const STACK: u32 = RECORD - 0x200;
pub const INNER_RECORD: u32 = RECORD - 0x40;
pub const INNER_INFO: u32 = INFO + 0x600;
pub const INNER_UNWIND: u32 = INFO + 0x640;
pub const TYPE: u32 = INFO + 0x700;
pub const TYPE_ARRAY: u32 = INFO + 0x720;
pub const CATCHABLE: u32 = INFO + 0x740;
pub const INNER_HANDLER: u32 = CODE + 0x80;
pub const INNER_ACTION: u32 = CODE + 0x90;

pub fn put(process: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    process.memory.write(u64::from(address), &bytes).unwrap();
}

pub fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn fixture_variant(two_frame: bool) -> Process32 {
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

const FAULT_RECORD: u32 = 0x1000_f100;
const FAULT_INNER: u32 = 0x1000_e100;
const FAULT_STACK: u32 = 0x1000_cd00;

fn retry_fixture(inner: bool, second_action: bool) -> Process32 {
    let mut p = fixture_variant(true);
    put(&mut p, FAULT_RECORD - 4, &[FAULT_STACK + 12]);
    put(&mut p, FAULT_RECORD, &[u32::MAX, CODE, 1]);
    put(&mut p, FAULT_INNER, &[FAULT_RECORD, INNER_HANDLER, 1]);
    put(&mut p, FAULT_STACK, &[CONTINUE, OBJECT, THROW_INFO]);
    let head = if inner { FAULT_INNER } else { FAULT_RECORD };
    let teb = p.cpu.fs_base();
    put(&mut p, teb, &[head]);
    p.cpu.set_register(Register32::Ebp, FAULT_RECORD + 12);
    p.cpu.set_register(Register32::Esp, FAULT_STACK);
    if !inner {
        put(&mut p, HANDLERS, &[0, 0, 0, CATCH]);
        put(&mut p, INFO, &[0x1993_0520, 5, UNWIND, 1, TRY, 0, 0]);
        put(&mut p, TRY, &[1, 3, 4, 1, HANDLERS]);
        let state = if second_action { 3 } else { 2 };
        put(&mut p, FAULT_RECORD + 8, &[state]);
        put(&mut p, UNWIND + 3 * 8, &[2, CLEANUP]);
        let last_action = if second_action { INNER_ACTION } else { CLEANUP };
        put(&mut p, UNWIND + 2 * 8, &[1, last_action]);
    }
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.run(2).instructions, 2);
    assert_eq!(p.cpu.eip, 0x7000_0ff4);
    p
}

fn fail_transition_twice(p: &mut Process32, destination: u32) {
    let destinations = [
        FAULT_INNER + 8,
        p.cpu.fs_base(),
        FAULT_RECORD + 8,
        FAULT_STACK - 4,
    ];
    let before_words = destinations.map(|address| word(p, address));
    let before_cpu = p.cpu;
    let page = u64::from(destination & !0xfff);
    p.memory.protect(page, 4096, Permissions::READ).unwrap();
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
                address: u64::from(destination),
                access: Access::Write
            }))
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before_cpu);
        assert_eq!(destinations.map(|address| word(p, address)), before_words);
    }
    p.memory
        .protect(page, 4096, Permissions::READ_WRITE)
        .unwrap();
}

pub fn outer_transition_faults_preserve_cleanup_and_catch_progress() {
    for second_action in [false, true] {
        for destination in [FAULT_RECORD + 8, FAULT_STACK - 4] {
            let mut p = retry_fixture(false, second_action);
            fail_transition_twice(&mut p, destination);
            let run = p.run(32);
            assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            assert_eq!(
                (run.instructions, run.api_calls),
                (if second_action { 6 } else { 4 }, 0)
            );
            let expected = if second_action { 2 } else { 1 };
            assert_eq!(word(&p, MARKER), expected);
            assert_eq!(p.cpu.register(Register32::Ecx), expected);
            assert_eq!(p.cpu.register(Register32::Esp), FAULT_STACK + 12);
            assert_eq!(word(&p, FAULT_RECORD + 8), 1);
        }
    }
}

pub fn inner_completion_faults_leave_every_transition_destination_unchanged() {
    for destination in [
        FAULT_INNER + 8,
        0x7ffd_e000,
        FAULT_RECORD + 8,
        FAULT_STACK - 4,
    ] {
        let mut p = retry_fixture(true, false);
        fail_transition_twice(&mut p, destination);
        let run = p.run(32);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (4, 0));
        assert_eq!(p.cpu.register(Register32::Ecx), 2);
        assert_eq!(p.cpu.register(Register32::Esp), FAULT_STACK + 12);
        assert_eq!(word(&p, FAULT_INNER + 8), u32::MAX);
        assert_eq!(word(&p, p.cpu.fs_base()), FAULT_RECORD);
    }
}
