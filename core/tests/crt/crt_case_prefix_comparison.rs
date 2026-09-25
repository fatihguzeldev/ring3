use super::case_prefix_compare_cases;

use case_prefix_compare_cases::{LEFT, RIGHT};
use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01d4;
const STACK: u32 = 0x1000_ff00;
const RETURN: u32 = 0x0040_1000;

fn process() -> Process32 {
    Process32::load(&case_prefix_compare_cases::executable(b"", b"", 0), 96).unwrap()
}

fn prepare(p: &mut Process32, left: u32, right: u32, count: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    let bytes: Vec<_> = [RETURN, left, right, count]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &bytes).unwrap();
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: i32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = RETURN;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 4);
    expected.set_register(Register32::Eax, value.cast_unsigned());
    assert_eq!(p.cpu, expected);
}

fn memory_fault(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn imported_case_prefix_comparison_runs_split_and_whole() {
    case_prefix_compare_cases::imported_results_across_budgets();
}

#[test]
fn readonly_inputs_overlap_and_zero_budget_preserve_state_errors_and_pages() {
    let mut p = process();
    p.memory.write(u64::from(LEFT), b"AaA\0").unwrap();
    p.memory.write(u64::from(RIGHT), b"aaa\0").unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    let pages = p.memory.mapped_pages();
    for (left, right, count, expected) in [(LEFT, RIGHT, 4, 0), (LEFT, LEFT + 1, 3, 97)] {
        let before = prepare(&mut p, left, right, count);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, expected);
        assert_eq!(p.memory.mapped_pages(), pages);
    }
    let mut bytes = [0; 4];
    p.memory.read(u64::from(LEFT), &mut bytes).unwrap();
    assert_eq!(&bytes, b"AaA\0");
    p.memory.read(u64::from(RIGHT), &mut bytes).unwrap();
    assert_eq!(&bytes, b"aaa\0");
}

#[test]
fn zero_count_skips_pointees_but_null_and_oversized_requests_refuse_then_retry() {
    let mut p = process();
    let before = prepare(&mut p, u32::MAX, 0x6000_0000, 0);
    success(&mut p, before, 0);
    for (left, right, count) in [
        (0, RIGHT, 0),
        (LEFT, 0, 1),
        (0, 0, 65537),
        (LEFT, RIGHT, u32::MAX),
    ] {
        let before = prepare(&mut p, left, right, count);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .write(u64::from(STACK + 12), &1_u32.to_le_bytes())
        .unwrap();
    let before = p.cpu;
    success(&mut p, before, 0);
}

#[test]
fn early_difference_nul_and_count_end_do_not_read_past_last_address() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (a, b, count, expected) in [
        (0, 0, 65536, 0),
        (b'Z', b'[', 2, 31),
        (b'A', b'a', 1, 0),
        (0, b'A', 2, -97),
    ] {
        p.memory.write(0xffff_ffff, &[a]).unwrap();
        p.memory.write(0x0040_2fff, &[b]).unwrap();
        let before = prepare(&mut p, u32::MAX, 0x0040_2fff, count);
        success(&mut p, before, expected);
    }
    p.memory.write(0xffff_ffff, b"A").unwrap();
    p.memory.write(u64::from(RIGHT), b"aa").unwrap();
    prepare(&mut p, u32::MAX, RIGHT, 2);
    memory_fault(&mut p);
    prepare(&mut p, RIGHT, u32::MAX, 2);
    memory_fault(&mut p);
}

#[test]
fn required_tail_and_read_permissions_fault_atomically_and_retry() {
    let mut p = process();
    p.memory.write(0x0040_2fff, b"A").unwrap();
    p.memory.write(u64::from(RIGHT), b"ab").unwrap();
    let before = prepare(&mut p, 0x0040_2fff, RIGHT, 2);
    memory_fault(&mut p);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x0040_3000, b"B").unwrap();
    p.memory
        .protect(0x0040_3000, 4096, Permissions::NONE)
        .unwrap();
    memory_fault(&mut p);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0);
    let before = prepare(&mut p, RIGHT, 0x0040_2fff, 2);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::NONE)
        .unwrap();
    memory_fault(&mut p);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0);
}

#[test]
fn full_bounded_unterminated_inputs_finish_and_include_the_last_byte() {
    let mut p = process();
    p.memory
        .map_zeroed(0x2000_0000, 131_072, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'A'; 65536]).unwrap();
    p.memory.write(0x2001_0000, &vec![b'a'; 65536]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2001_0000, 65536);
    success(&mut p, before, 0);
    p.memory.write(0x2001_ffff, b"B").unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2001_0000, 65536);
    success(&mut p, before, -1);
}

#[test]
fn whole_cdecl_frame_is_validated_before_comparison() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let words: Vec<_> = [RETURN, LEFT, RIGHT, 0]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(0xffff_fff0, &words).unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    memory_fault(&mut p);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    let before = p.cpu;
    success(&mut p, before, 0);
}
