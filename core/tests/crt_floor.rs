#[path = "support/crt_floor_cases.rs"]
mod crt_floor_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/integer_cpu_state.rs"]
mod integer_cpu_state;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01d0;
const CODE: u32 = 0x0040_1000;
const DATA: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[
                0xdd, 0x05, 0x80, 0x21, 0x40, 0, 0xdd, 0x1d, 0x88, 0x21, 0x40, 0, 0xdf, 0xe0, 0xcc,
                0xdc, 0x35, 0x90, 0x21, 0x40, 0,
            ],
            "MSVCRT.dll",
            &["floor"],
        ),
        96,
    )
    .unwrap()
}

fn write_words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn prepare(p: &mut Process32, input: f64) {
    let bytes = input.to_le_bytes();
    write_words(
        p,
        STACK,
        &[
            CODE + 6,
            u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            u32::from_le_bytes(bytes[4..].try_into().unwrap()),
        ],
    );
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
}

fn pop(p: &mut Process32) -> u64 {
    p.cpu.set_x87_control_word(0x027f);
    p.cpu.eip = CODE + 6;
    assert_eq!(p.run(1).instructions, 1);
    let mut bytes = [0; 8];
    p.memory.read(u64::from(DATA + 8), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn refuses(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn imported_floor_returns_double_in_x87_and_leaves_cdecl_cleanup_to_caller() {
    crt_floor_cases::finite_results_across_budgets();
}

#[test]
fn floor_ignores_precision_rounding_and_preserves_caller_state_and_lower_stack() {
    for pc in [0, 0x200, 0x300] {
        for rc in [0, 0x400, 0x800, 0xc00] {
            let mut p = process();
            p.memory
                .write(u64::from(DATA), &42_f64.to_le_bytes())
                .unwrap();
            assert_eq!(p.run(1).instructions, 1);
            prepare(&mut p, -2.75);
            p.cpu.set_x87_control_word(0x7f | pc | rc);
            p.cpu.eflags = 0xced7;
            p.cpu.set_register(Register32::Eax, 99);
            for page in [0x7000_2000, 0x7ffd_e000] {
                p.memory.protect(page, 4096, Permissions::NONE).unwrap();
            }
            let pages = p.memory.mapped_pages();
            let before = p.cpu;
            assert_eq!(p.run(0).api_calls, 0);
            assert_eq!(p.cpu, before);
            assert_eq!(p.run(1).api_calls, 1);
            let mut expected = before;
            expected.eip = CODE + 6;
            expected.set_register(Register32::Esp, STACK + 4);
            integer_cpu_state::assert_unchanged(&p.cpu, &expected);
            assert_eq!(p.memory.mapped_pages(), pages);
            assert_eq!(pop(&mut p), (-3_f64).to_bits());
            assert_eq!(pop(&mut p), 42_f64.to_bits());
        }
    }
}

#[test]
fn nonfinite_inputs_unmasked_control_and_full_stack_refuse_then_retry() {
    let mut p = process();
    for input in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        prepare(&mut p, input);
        refuses(&mut p);
    }
    prepare(&mut p, 1.75);
    p.cpu.set_x87_control_word(0x027e);
    refuses(&mut p);
    p.cpu.set_x87_control_word(0x027f);
    p.memory
        .write(u64::from(DATA), &42_f64.to_le_bytes())
        .unwrap();
    for _ in 0..8 {
        p.cpu.eip = CODE;
        assert_eq!(p.run(1).instructions, 1);
    }
    prepare(&mut p, 1.75);
    refuses(&mut p);
    assert_eq!(pop(&mut p), 42_f64.to_bits());
    prepare(&mut p, 1.75);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(pop(&mut p), 1_f64.to_bits());
    for _ in 0..7 {
        assert_eq!(pop(&mut p), 42_f64.to_bits());
    }
}

#[test]
fn incomplete_or_unreadable_argument_frame_is_atomic_and_retryable() {
    let mut p = process();
    let stack = 0x0040_2ff8;
    write_words(&mut p, stack, &[CODE + 6, 0]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, stack);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    write_words(&mut p, 0x0040_3000, &[0xbffc_0000]);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::NONE)
        .unwrap();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Esp), stack + 4);
    assert_eq!(pop(&mut p), (-2_f64).to_bits());
}

#[test]
fn result_push_preserves_sticky_precision_and_clears_round_up() {
    let mut p = process();
    p.memory
        .write(u64::from(DATA), &32_f64.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(DATA + 16), &13_f64.to_le_bytes())
        .unwrap();
    assert_eq!(p.run(1).instructions, 1);
    p.cpu.eip = CODE + 15;
    assert_eq!(p.run(1).instructions, 1);
    p.cpu.eip = CODE + 12;
    assert_eq!(p.run(1).instructions, 1);
    assert_eq!(p.cpu.register(Register32::Eax), 0x3a20);
    prepare(&mut p, 1.75);
    assert_eq!(p.run(1).api_calls, 1);
    p.cpu.eip = CODE + 12;
    assert_eq!(p.run(1).instructions, 1);
    assert_eq!(p.cpu.register(Register32::Eax), 0x3020);
    assert_eq!(pop(&mut p), 1_f64.to_bits());
    assert_eq!(pop(&mut p), 0x4003_b13b_13b1_3b14);
}

#[test]
fn end_of_address_space_frame_is_checked_before_return_publication() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    write_words(&mut p, 0xffff_fff8, &[CODE + 6, 0]);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff8);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    write_words(&mut p, 0xffff_fff4, &[CODE + 6, 0, 0x3ffc_0000]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Esp), 0xffff_fff8);
    assert_eq!(pop(&mut p), 1_f64.to_bits());
}

#[test]
fn scheduled_child_returns_floor_on_its_own_x87_stack() {
    let mut p = Process32::load(&crt_floor_cases::executable((-2.75_f64).to_bits()), 96).unwrap();
    write_words(&mut p, STACK, &[CODE, 0, 0, CODE, 0, 4, 0]);
    p.cpu.eip = 0x7000_0548;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    let child = p.cpu.register(Register32::Eax);
    assert_ne!(child, 0);
    write_words(&mut p, STACK, &[CODE, child]);
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.eip = 0x7000_0550;
    assert_eq!(p.run(1).api_calls, 1);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    assert_eq!(run.api_calls, 1);
    let mut bytes = [0; 8];
    p.memory
        .read(u64::from(crt_floor_cases::RESULT), &mut bytes)
        .unwrap();
    assert_eq!(u64::from_le_bytes(bytes), (-3_f64).to_bits());
}
