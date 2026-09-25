use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_012c;
const STACK: u32 = 0x1000_ff00;
const DATA: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_mbsrchr"]),
        48,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, pointer: u32, character: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, pointer, character].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn success(process: &mut Process32, before: Cpu32, pointer: u32) {
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let mut expected = before;
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, pointer);
    assert_eq!(process.cpu, expected);
}

#[test]
fn last_match_including_nul_obeys_single_byte_conversion() {
    let mut process = process();
    let mut bytes: Vec<u8> = (1..=255).chain((1..=255).rev()).collect();
    bytes.extend_from_slice(&[0, 42, 42, 0]);
    process.memory.write(u64::from(DATA), &bytes).unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    process
        .memory
        .write(0x7000_2020, &123_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    for character in (0..=255).chain([0x12a, 0x100, u32::MAX]) {
        let byte = character.to_le_bytes()[0];
        let offset = bytes[..511]
            .iter()
            .rposition(|&value| value == byte)
            .unwrap();
        let before = prepare(&mut process, DATA, character);
        success(&mut process, before, DATA + u32::try_from(offset).unwrap());
    }
    let mut unchanged = vec![0; bytes.len()];
    process
        .memory
        .read(u64::from(DATA), &mut unchanged)
        .unwrap();
    assert_eq!(unchanged, bytes);
    let mut errno = [0; 4];
    process.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 123);
    assert_eq!(process.last_error().unwrap(), 77);
    for (pointer, character, result) in [(DATA + 510, 42, 0), (DATA + 510, 0, DATA + 510)] {
        let before = prepare(&mut process, pointer, character);
        success(&mut process, before, result);
    }
}

#[test]
fn scans_page_boundaries_without_reading_beyond_the_terminator() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_2ffe, b"a.a.\0").unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE * 2, Permissions::READ)
        .unwrap();
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut process, 0x0040_2ffe, 46);
    success(&mut process, before, 0x0040_3001);
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0xffff_fffe, b"x\0").unwrap();
    for (character, result) in [(0, u32::MAX), (120, u32::MAX - 1), (46, 0)] {
        let before = prepare(&mut process, u32::MAX - 1, character);
        success(&mut process, before, result);
    }
}

#[test]
fn bad_memory_and_incomplete_cdecl_frames_are_atomic() {
    for pointer in [0x0040_2ffe, 0x7000_0000, u32::MAX] {
        let mut process = process();
        if pointer == u32::MAX {
            process
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            process.memory.write(u64::from(pointer), b".").unwrap();
        } else if pointer == 0x0040_2ffe {
            process.memory.write(u64::from(pointer), b"..").unwrap();
        }
        let before = prepare(&mut process, pointer, 46);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    let mut process = process();
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
    prepare(&mut process, DATA, 0);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    prepare(&mut process, DATA, 0);
    process.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}

#[test]
fn unterminated_limit_and_null_are_explicitly_unsupported() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .write(0x2000_0000, &vec![b'.'; 65536])
        .unwrap();
    for pointer in [0, 0x2000_0000] {
        let before = prepare(&mut process, pointer, 46);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    process.memory.write(0x2000_ffff, &[0]).unwrap();
    let before = prepare(&mut process, 0x2000_0000, 46);
    success(&mut process, before, 0x2000_fffe);
    let before = prepare(&mut process, 0x2000_0000, 0);
    success(&mut process, before, 0x2000_ffff);
}

#[test]
fn source_can_alias_the_captured_arguments() {
    let mut process = process();
    let before = prepare(&mut process, STACK + 8, 46);
    success(&mut process, before, STACK + 8);
    let mut frame = [0; 12];
    process.memory.read(u64::from(STACK), &mut frame).unwrap();
    assert_eq!(&frame[8..], &46_u32.to_le_bytes());
}
