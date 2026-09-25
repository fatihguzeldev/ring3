use super::imported_executable;

use super::interlocked_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0200;
const STACK: u32 = 0x1000_ff00;
const TARGET: u32 = 0x0040_2180;

#[test]
fn imported_guest_exchange_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut process = Process32::load(&interlocked_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = process.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (11, 2));
        assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x8000_0000);
        assert_eq!(process.cpu.register(Register32::Edi), 42);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["InterlockedExchange"]),
        32,
    )
    .unwrap()
}

fn put(process: &mut Process32, address: u32, value: u32) {
    process
        .memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(process: &mut Process32, target: u32, value: u32) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, target, value].into_iter().enumerate() {
        put(process, STACK + u32::try_from(index).unwrap() * 4, value);
    }
}

#[test]
fn exchange_returns_prior_bits_and_changes_one_word_in_one_api_step() {
    let mut process = process();
    put(&mut process, 0x7ffd_e034, 77);
    put(&mut process, TARGET - 4, 123);
    put(&mut process, TARGET + 4, 456);
    for (old, value) in [
        (0, 1),
        (1, u32::MAX),
        (u32::MAX, 0x8000_0000),
        (0x8000_0000, 0x8000_0000),
    ] {
        put(&mut process, TARGET, old);
        prepare(&mut process, TARGET, value);
        let mut expected = process.cpu;
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 12);
        expected.set_register(Register32::Eax, old);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu, expected);
        assert_eq!(word(&process, TARGET), value);
        assert_eq!(word(&process, TARGET - 4), 123);
        assert_eq!(word(&process, TARGET + 4), 456);
        assert_eq!(process.last_error().unwrap(), 77);
    }
}

#[test]
fn exchange_faults_and_unsupported_alignment_leave_cpu_and_memory_unchanged() {
    let mut process = process();
    put(&mut process, TARGET, 42);
    let pages = process.memory.mapped_pages();
    for permissions in [
        Permissions::READ,
        Permissions {
            read: false,
            write: true,
            execute: false,
        },
    ] {
        process
            .memory
            .protect(0x0040_2000, PAGE_SIZE, permissions)
            .unwrap();
        prepare(&mut process, TARGET, 99);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        process
            .memory
            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(word(&process, TARGET), 42);
    }
    for target in [0, 0x6000_0000] {
        prepare(&mut process, target, 99);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
    }
    for target in [TARGET + 1, u32::MAX] {
        prepare(&mut process, target, 99);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, TARGET), 42);
    }
    prepare(&mut process, TARGET, 99);
    process.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(word(&process, TARGET), 42);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn aliases_use_captured_arguments_and_reread_the_saved_return() {
    let mut process = process();
    for (target, value, old, resume) in [
        (STACK + 4, 0x1234, STACK + 4, 0x0040_1000),
        (STACK, 0x0040_1080, 0x0040_1000, 0x0040_1080),
        (0x7ffd_e034, 99, 77, 0x0040_1000),
    ] {
        put(&mut process, 0x7ffd_e034, 77);
        prepare(&mut process, target, value);
        let result = process.run(1);
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.eip, resume);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
        assert_eq!(process.cpu.register(Register32::Eax), old);
        assert_eq!(word(&process, target), value);
    }
}

#[test]
fn final_guest32_word_is_valid_and_zero_budget_never_exchanges() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    put(&mut process, 0xffff_fffc, 0x8000_0000);
    prepare(&mut process, 0xffff_fffc, 42);
    let before = process.cpu;
    assert_eq!(process.run(0).api_calls, 0);
    assert_eq!(process.cpu, before);
    assert_eq!(word(&process, 0xffff_fffc), 0x8000_0000);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), 0x8000_0000);
    assert_eq!(word(&process, 0xffff_fffc), 42);
}
