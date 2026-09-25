use super::crt_getenv_cases;

use crt_getenv_cases::{ENVIRON, NAME, word};
use ring3_core::execution::{
    Cpu32, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_01dc;
const STACK: u32 = 0x1000_ff00;
const RETURN: u32 = 0x0040_1000;
const TABLE: u32 = 0x0040_2400;
const ENTRY: u32 = 0x0040_2500;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn process() -> Process32 {
    Process32::load_with_options(
        &crt_getenv_cases::executable(b"demo"),
        128,
        ProcessOptions {
            environment: &[b"Demo=start"],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, source: u32) -> Cpu32 {
    words(p, STACK, &[RETURN, source]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = RETURN;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

fn failure(p: &mut Process32, unsupported: bool) {
    let before = p.cpu;
    let run = p.run(1);
    if unsupported {
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    } else {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn imported_getenv_returns_existing_value_pointer_across_budgets() {
    crt_getenv_cases::imported_lookup_across_budgets();
}

#[test]
fn guest_environment_replacement_and_value_edits_leave_win32_snapshot_separate() {
    let mut p = process();
    let original = word(&p, word(&p, ENVIRON)) + 5;
    let before = prepare(&mut p, NAME);
    success(&mut p, before, original);
    p.memory.write(u64::from(original), b"other\0").unwrap();
    let before = prepare(&mut p, NAME);
    success(&mut p, before, original);
    p.memory.write(u64::from(ENTRY), b"Demo=guest\0").unwrap();
    words(&mut p, TABLE, &[ENTRY, 0]);
    words(&mut p, ENVIRON, &[TABLE]);
    let before = prepare(&mut p, NAME);
    success(&mut p, before, ENTRY + 5);
    words(&mut p, STACK, &[RETURN, NAME, ENTRY + 32, 16]);
    p.cpu.eip = 0x7000_0240;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), 5);
    let mut value = [0; 6];
    p.memory.read(u64::from(ENTRY + 32), &mut value).unwrap();
    assert_eq!(&value, b"start\0");
    words(&mut p, ENVIRON, &[0]);
    let before = prepare(&mut p, NAME);
    success(&mut p, before, 0);
}

#[test]
fn readonly_lookup_and_empty_name_preserve_cpu_errors_and_pages_without_teb_access() {
    let mut p = process();
    let expected = word(&p, word(&p, ENVIRON)) + 5;
    words(&mut p, 0x7000_2020, &[88]);
    words(&mut p, 0x7ffd_e034, &[77]);
    for page in [0x0040_2000, 0x7000_2000, 0x7000_4000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.cpu.set_fs_base(u32::MAX);
    let pages = p.memory.mapped_pages();
    let before = prepare(&mut p, NAME);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    success(&mut p, before, expected);
    assert_eq!(word(&p, 0x7000_2020), 88);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, NAME + 4);
    success(&mut p, before, 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(word(&p, 0x7ffd_e034), 77);
}

#[test]
fn query_table_and_entry_faults_repair_on_the_same_call() {
    let mut p = process();
    let before = prepare(&mut p, 0x0040_3000);
    failure(&mut p, false);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x0040_3000, b"demo\0").unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    failure(&mut p, false);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, ENVIRON, &[0x0040_4000]);
    failure(&mut p, false);
    p.memory
        .map_zeroed(0x0040_4000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x0040_4000, &[0x0040_5000, 0]);
    failure(&mut p, false);
    p.memory
        .map_zeroed(0x0040_5000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x0040_5000, b"Demo=ready\0").unwrap();
    p.memory
        .protect(0x0040_5000, 4096, Permissions::NONE)
        .unwrap();
    failure(&mut p, false);
    p.memory
        .protect(0x0040_5000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0x0040_5005);
}

#[test]
fn early_match_needs_neither_value_bytes_nor_later_entries() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(NAME), b"A\0").unwrap();
    p.memory.write(0x0040_2ffe, b"a=").unwrap();
    words(&mut p, u32::MAX - 3, &[0x0040_2ffe]);
    words(&mut p, ENVIRON, &[u32::MAX - 3]);
    let before = prepare(&mut p, NAME);
    success(&mut p, before, 0x0040_3000);
    p.memory.write(0x0040_2ffe, b"?").unwrap();
    prepare(&mut p, NAME);
    failure(&mut p, false);
    words(&mut p, ENVIRON, &[TABLE]);
    words(&mut p, TABLE, &[u32::MAX, 0]);
    p.memory.write(u64::from(u32::MAX), b"?").unwrap();
    let before = prepare(&mut p, NAME);
    success(&mut p, before, 0);
    p.memory.write(u64::from(u32::MAX), b"A").unwrap();
    prepare(&mut p, NAME);
    failure(&mut p, false);
    words(&mut p, TABLE, &[u32::MAX - 1, 0]);
    p.memory.write(u64::from(u32::MAX - 1), b"A=").unwrap();
    prepare(&mut p, NAME);
    failure(&mut p, false);
}

#[test]
fn table_limit_allows_256_entries_and_requires_a_terminator_for_misses() {
    let mut p = process();
    p.memory
        .map_zeroed(0x2000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(ENTRY), b"other=x\0").unwrap();
    words(&mut p, 0x2000_0000, &[ENTRY; 257]);
    words(&mut p, ENVIRON, &[0x2000_0000]);
    prepare(&mut p, NAME);
    failure(&mut p, true);
    words(&mut p, 0x2000_0400, &[0]);
    let before = p.cpu;
    success(&mut p, before, 0);
    p.memory
        .write(u64::from(ENTRY + 32), b"demo=last\0")
        .unwrap();
    words(&mut p, 0x2000_03fc, &[ENTRY + 32]);
    let before = prepare(&mut p, NAME);
    success(&mut p, before, ENTRY + 37);
}

#[test]
fn query_profile_and_cdecl_frame_checks_are_bounded_and_atomic() {
    let mut p = process();
    prepare(&mut p, 0);
    failure(&mut p, true);
    for invalid in [b"A=B\0".as_slice(), b"\x80\0"] {
        p.memory.write(u64::from(NAME), invalid).unwrap();
        prepare(&mut p, NAME);
        failure(&mut p, true);
    }
    p.memory
        .map_zeroed(0x2000_0000, 32768, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'A'; 32768]).unwrap();
    prepare(&mut p, 0x2000_0000);
    failure(&mut p, true);
    p.memory.write(0x2000_7fff, &[0]).unwrap();
    let before = p.cpu;
    success(&mut p, before, 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff8, &[RETURN, 0x2000_0000]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fffc);
    failure(&mut p, false);
    p.cpu.set_register(Register32::Esp, 0xffff_fff8);
    let before = p.cpu;
    success(&mut p, before, 0);
}
