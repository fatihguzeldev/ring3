#[path = "support/crt_setjmp_cases.rs"]
mod crt_setjmp_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use crt_setjmp_cases::{ENV, OPTIONAL, REGISTRATION, SAVED};
use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01d8;
const STACK: u32 = 0x1000_ff00;
const RETURN: u32 = 0x0040_1000;
const WRITE_ONLY: Permissions = Permissions {
    write: true,
    ..Permissions::NONE
};

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn read(p: &Process32, address: u32, count: usize) -> Vec<u32> {
    let mut bytes = vec![0; count * 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect()
}

fn process() -> Process32 {
    let exe = imported_executable::pe32(
        &[0xdd, 0x05, 0x80, 0x21, 0x40, 0, 0xcc],
        "MSVCRT.dll",
        &["_setjmp3"],
    );
    let mut p = Process32::load(&exe, 96).unwrap();
    p.memory.write(0x0040_2180, &42_f64.to_le_bytes()).unwrap();
    assert_eq!(p.run(1).instructions, 1);
    words(&mut p, ENV, &[0xa5a5_a5a5; 16]);
    for (register, value) in SAVED {
        p.cpu.set_register(register, value);
    }
    p.cpu.set_register(Register32::Ecx, 99);
    p.cpu.set_register(Register32::Edx, 88);
    p.cpu.set_x87_control_word(0x0e7f);
    p.cpu.eflags = 0xced7;
    p
}

fn prepare(p: &mut Process32, env: u32, count: u32) -> Cpu32 {
    let mut frame = vec![RETURN, env, count];
    frame.extend(OPTIONAL);
    words(p, STACK, &frame);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = RETURN;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 4);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(p.cpu, expected);
}

fn failure(p: &mut Process32, unsupported: bool) {
    let before = p.cpu;
    let output = read(p, ENV, 16);
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
    assert_eq!(read(p, ENV, 16), output);
}

fn active(p: &mut Process32) {
    words(p, p.cpu.fs_base(), &[REGISTRATION]);
    words(p, REGISTRATION + 12, &[7]);
}

#[test]
fn imported_capture_records_layout_and_cdecl_cleanup_native_and_wasm() {
    crt_setjmp_cases::imported_capture_across_budgets();
}

#[test]
fn write_only_unaligned_output_preserves_unused_tail_state_errors_and_pages() {
    let mut p = process();
    p.memory
        .map_zeroed(0x2000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &[0xa5; 4096]).unwrap();
    p.memory.protect(0x2000_0000, 4096, WRITE_ONLY).unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let pages = p.memory.mapped_pages();
    let before = prepare(&mut p, 0x2000_0001, 0);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    success(&mut p, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x2000_0000, 4096, Permissions::READ)
        .unwrap();
    let output = read(&p, 0x2000_0001, 16);
    assert_eq!(output[4], STACK);
    assert_eq!(&output[6..10], &[u32::MAX, u32::MAX, 0x5643_3230, 0]);
    assert_eq!(&output[10..], &[0xa5a5_a5a5; 6]);
    assert_eq!(p.last_error().unwrap(), 0);
}

#[test]
fn malformed_counts_null_and_frame_overlap_refuse_before_publication() {
    let mut p = process();
    for (env, count) in [
        (0, 0),
        (ENV, 9),
        (ENV, u32::MAX),
        (STACK, 0),
        (STACK - 39, 0),
        (STACK + 40, 8),
    ] {
        let before = prepare(&mut p, env, count);
        let frame = read(&p, STACK, 11);
        failure(&mut p, true);
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, STACK, 11), frame);
    }
    let before = prepare(&mut p, STACK - 40, 0);
    success(&mut p, before);
    let before = prepare(&mut p, ENV, 9);
    failure(&mut p, true);
    words(&mut p, STACK + 8, &[0]);
    success(&mut p, before);
}

#[test]
fn variable_frame_reads_exact_count_and_can_retry_a_missing_tail() {
    let mut p = process();
    words(&mut p, 0x0040_2ff4, &[RETURN, ENV, 0]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0x0040_2ff4);
    let before = p.cpu;
    success(&mut p, before);
    words(&mut p, 0x0040_2ff4, &[RETURN, ENV, 8]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0x0040_2ff4);
    let before = p.cpu;
    failure(&mut p, false);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x0040_3000, &OPTIONAL);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::NONE)
        .unwrap();
    failure(&mut p, false);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before);
}

#[test]
fn written_output_tail_fault_is_atomic_and_unused_slots_need_no_access() {
    let mut p = process();
    active(&mut p);
    p.memory.write(0x0040_2fe0, &[0xa5; 32]).unwrap();
    let before = prepare(&mut p, 0x0040_2fe0, 8);
    failure(&mut p, false);
    assert_eq!(read(&p, 0x0040_2fe0, 8), vec![0xa5a5_a5a5; 8]);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    failure(&mut p, false);
    assert_eq!(read(&p, 0x0040_2fe0, 8), vec![0xa5a5_a5a5; 8]);
    p.memory.protect(0x0040_3000, 4096, WRITE_ONLY).unwrap();
    success(&mut p, before);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, 0x0040_2fd8, 0);
    success(&mut p, before);
}

#[test]
fn thread_head_and_legacy_fallback_fault_before_output_and_repair() {
    let mut p = process();
    let before = prepare(&mut p, ENV, 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    failure(&mut p, false);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x7ffd_e000, &[0x0040_2ff4]);
    failure(&mut p, false);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x0040_3000, &[5]);
    success(&mut p, before);
    assert_eq!(read(&p, ENV + 28, 1), vec![5]);
    words(&mut p, 0x7ffd_e000, &[u32::MAX - 4]);
    prepare(&mut p, ENV, 1);
    failure(&mut p, false);
    let before = prepare(&mut p, ENV, 2);
    success(&mut p, before);
    p.cpu.set_fs_base(u32::MAX - 1);
    prepare(&mut p, ENV, 0);
    failure(&mut p, false);
}

#[test]
fn address_space_end_checks_frame_and_only_written_output_prefix() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut p, 0xffff_ffd8, 0);
    success(&mut p, before);
    let prefix = read(&p, 0xffff_fff0, 4);
    prepare(&mut p, 0xffff_fff0, 0);
    failure(&mut p, false);
    assert_eq!(read(&p, 0xffff_fff0, 4), prefix);
    words(&mut p, 0xffff_fff4, &[RETURN, ENV, 0]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff8);
    failure(&mut p, false);
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    let before = p.cpu;
    success(&mut p, before);
}

#[test]
fn scheduled_child_captures_its_own_exception_head() {
    let mut p = Process32::load(&crt_setjmp_cases::executable(3).0, 96).unwrap();
    words(&mut p, STACK, &[RETURN, 0, 0, RETURN, 0, 4, 0]);
    p.cpu.eip = 0x7000_0548;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    let child = p.cpu.register(Register32::Eax);
    assert_ne!(child, 0);
    words(&mut p, 0x1101_0000, &[REGISTRATION]);
    words(&mut p, STACK, &[RETURN, child]);
    p.cpu.eip = 0x7000_0550;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(run.api_calls, 1);
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    assert_eq!(read(&p, ENV + 24, 1), vec![REGISTRATION]);
    assert_eq!(read(&p, 0x7ffd_e000, 1), vec![u32::MAX]);
}
