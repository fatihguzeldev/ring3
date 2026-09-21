#[path = "support/imported_executable.rs"]
mod imported_executable;

#[path = "support/counter_executable.rs"]
mod counter_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const STACK: u32 = 0x1000_ff00;
const TARGET: u32 = 0x0040_2180;
const INCREMENT: u32 = 0x7000_0204;
const DECREMENT: u32 = 0x7000_0208;

#[test]
fn imported_counter_pair_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&counter_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (7, 2));
        assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Ebx), 0);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(word(&p, TARGET), u32::MAX);
    }
}

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "KERNEL32.dll",
            &["InterlockedIncrement", "InterlockedDecrement"],
        ),
        32,
    )
    .unwrap()
}

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, target: u32) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    put(p, STACK, 0x0040_1000);
    put(p, STACK + 4, target);
}

#[test]
fn counter_operations_return_new_wrapping_values_and_preserve_other_state() {
    let mut p = process();
    put(&mut p, TARGET - 4, 123);
    put(&mut p, TARGET + 4, 456);
    put(&mut p, 0x7ffd_e034, 77);
    for (api, delta) in [(INCREMENT, 1), (DECREMENT, u32::MAX)] {
        for old in [0_u32, 1, u32::MAX, 0x7fff_ffff, 0x8000_0000] {
            put(&mut p, TARGET, old);
            prepare(&mut p, api, TARGET);
            let mut expected = p.cpu;
            expected.eip = 0x0040_1000;
            expected.set_register(Register32::Esp, STACK + 8);
            expected.set_register(Register32::Eax, old.wrapping_add(delta));
            let result = p.run(1);
            assert_eq!((result.instructions, result.api_calls), (0, 1));
            assert_eq!(p.cpu, expected);
            assert_eq!(word(&p, TARGET), old.wrapping_add(delta));
            assert_eq!(word(&p, TARGET - 4), 123);
            assert_eq!(word(&p, TARGET + 4), 456);
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
}

#[test]
fn faults_alignment_and_zero_budget_leave_cpu_and_word_unchanged() {
    for api in [INCREMENT, DECREMENT] {
        let mut p = process();
        put(&mut p, TARGET, 42);
        for permissions in [
            Permissions::READ,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        ] {
            p.memory
                .protect(0x0040_2000, PAGE_SIZE, permissions)
                .unwrap();
            prepare(&mut p, api, TARGET);
            let before = p.cpu;
            assert_eq!(p.run(0).api_calls, 0);
            assert_eq!(p.cpu, before);
            let result = p.run(1);
            assert!(matches!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!(result.api_calls, 0);
            assert_eq!(p.cpu, before);
            p.memory
                .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            assert_eq!(word(&p, TARGET), 42);
        }
        for target in [0, 0x6000_0000, TARGET + 1, u32::MAX] {
            prepare(&mut p, api, target);
            let before = p.cpu;
            let result = p.run(1);
            if target.is_multiple_of(4) {
                assert!(matches!(
                    result.reason,
                    ProcessStop::Stopped(StopReason::MemoryFault(_))
                ));
            } else {
                assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
            }
            assert_eq!(result.api_calls, 0);
            assert_eq!(p.cpu, before);
        }
        prepare(&mut p, api, TARGET);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, TARGET), 42);
    }
}

#[test]
fn top_word_and_frame_error_aliases_keep_captured_target_and_return_reread() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for (api, delta) in [(INCREMENT, 1_u32), (DECREMENT, u32::MAX)] {
        for target in [0xffff_fffc, STACK, STACK + 4, 0x7ffd_e034, 0x7000_2020] {
            put(&mut p, target, 77);
            prepare(&mut p, api, target);
            let old = word(&p, target);
            let next = old.wrapping_add(delta);
            assert_eq!(p.run(1).api_calls, 1);
            assert_eq!(p.cpu.register(Register32::Eax), next);
            assert_eq!(p.cpu.register(Register32::Esp), STACK + 8);
            assert_eq!(p.cpu.eip, if target == STACK { next } else { 0x0040_1000 });
            assert_eq!(word(&p, target), next);
        }
    }
}
