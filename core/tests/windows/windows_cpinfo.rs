use super::cpinfo_executable;
use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0090;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["GetCPInfo"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, page: u32, output: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, page, output].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn success(process: &mut Process32, before: Cpu32, value: u32, target: u32) {
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let mut expected = before;
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 12);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
}

fn fields() -> [u8; 18] {
    let mut expected = [0; 18];
    expected[0] = 1;
    expected[4] = b'?';
    expected
}

#[test]
fn single_byte_metadata_writes_only_fields_for_explicit_pages_and_aliases() {
    for page in [0, 1, 3, 437, 1252] {
        let mut process = process();
        process.memory.write(0x0040_2180, &[0xa5; 20]).unwrap();
        process
            .memory
            .write(0x7ffd_e034, &77_u32.to_le_bytes())
            .unwrap();
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        process
            .memory
            .protect(
                0x0040_2000,
                PAGE_SIZE,
                Permissions {
                    read: false,
                    write: true,
                    execute: false,
                },
            )
            .unwrap();
        let before = prepare(&mut process, page, 0x0040_2180);
        success(&mut process, before, 1, 0x0040_1000);
        process
            .memory
            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        let mut bytes = [0; 20];
        process.memory.read(0x0040_2180, &mut bytes).unwrap();
        assert_eq!(&bytes[..18], &fields());
        assert_eq!(&bytes[18..], &[0xa5; 2]);
        assert_eq!(process.last_error().unwrap(), 77);
    }
}

#[test]
fn cross_page_and_top_address_fields_have_exact_write_bounds() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for address in [0x0040_2ff8, 0xffff_ffee] {
        process
            .memory
            .write(u64::from(address), &[0xa5; 18])
            .unwrap();
        let before = prepare(&mut process, 437, address);
        success(&mut process, before, 1, 0x0040_1000);
        let mut bytes = [0; 18];
        process.memory.read(u64::from(address), &mut bytes).unwrap();
        assert_eq!(bytes, fields());
    }
    for address in [0x0040_2ff8, 0xffff_fff0] {
        process
            .memory
            .write(u64::from(address), &[0xa5; 8])
            .unwrap();
        process
            .memory
            .protect(0x0040_3000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        let before = prepare(&mut process, 1252, address);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        let mut bytes = [0; 8];
        process.memory.read(u64::from(address), &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 8]);
    }
}

#[test]
fn null_unknown_pages_and_faulting_frames_have_distinct_failures() {
    let mut process = process();
    for page in [437, u32::MAX] {
        let before = prepare(&mut process, page, 0);
        success(&mut process, before, 0, 0x0040_1000);
    }
    assert_eq!(process.last_error().unwrap(), 87);
    for page in [2, 42, 932, 65001, u32::MAX] {
        let before = prepare(&mut process, page, u32::MAX);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 87);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, 437, 0);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    prepare(&mut process, 437, 0x0040_2180);
    process.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}

#[test]
fn output_can_overwrite_captured_arguments_saved_return_and_last_error() {
    let mut process = process();
    let before = prepare(&mut process, 437, STACK);
    success(&mut process, before, 1, 1);
    let mut bytes = [0; 18];
    process.memory.read(u64::from(STACK), &mut bytes).unwrap();
    assert_eq!(bytes, fields());
    let before = prepare(&mut process, 1252, 0x7ffd_e034);
    success(&mut process, before, 1, 0x0040_1000);
    assert_eq!(process.last_error().unwrap(), 1);
}

#[test]
fn imported_code_page_metadata_guest_resumes_consistently() {
    let bytes = cpinfo_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
    let result = whole.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 2));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..30 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions, result.api_calls)
    );
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Esp), stack);
    assert_eq!(whole.cpu.register(Register32::Eax), 1);
    assert_eq!(whole.cpu.register(Register32::Ebx), 1);
    assert_eq!(whole.cpu.register(Register32::Edx), 63);
    for process in [&whole, &stepped] {
        let mut bytes = [0; 18];
        process.memory.read(0x0040_2180, &mut bytes).unwrap();
        assert_eq!(bytes, fields());
    }
}
