use super::imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0130;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_mbsinc"]),
        26,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, pointer: u32) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK) + 4, &pointer.to_le_bytes())
        .unwrap();
}

#[test]
fn every_single_byte_including_nul_advances_to_an_unmapped_next_address() {
    let mut process = process();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for byte in 0..=255 {
        process
            .memory
            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        process.memory.write(0x0040_2fff, &[byte]).unwrap();
        process
            .memory
            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        prepare(&mut process, 0x0040_2fff);
        let mut expected = process.cpu;
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 4);
        expected.set_register(Register32::Eax, 0x0040_3000);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu, expected);
    }
}

#[test]
fn null_read_fault_overflow_and_incomplete_frames_are_atomic() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for pointer in [0, 0x7000_0000, u32::MAX] {
        prepare(&mut process, pointer);
        let before = process.cpu;
        let result = process.run(1);
        if pointer == 0 {
            assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        } else {
            assert!(matches!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    prepare(&mut process, 0x0040_2180);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    prepare(&mut process, STACK + 4);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), STACK + 5);
}
