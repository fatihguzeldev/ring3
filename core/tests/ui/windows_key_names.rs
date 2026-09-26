use super::imported_executable;

use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0700;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2200;

fn process() -> Process32 {
    let process = Process32::load(
        &imported_executable::pe32(&[0xcc], "USER32.dll", &["GetKeyNameTextA"]),
        32,
    )
    .unwrap();
    let mut address = [0; 4];
    process.memory.read(0x0040_2060, &mut address).unwrap();
    assert_eq!(u32::from_le_bytes(address), API);
    process
}

fn prepare(process: &mut Process32, parameter: u32, output: u32, size: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000_u32, parameter, output, size]
        .into_iter()
        .enumerate()
    {
        process
            .memory
            .write(u64::from(STACK) + (index * 4) as u64, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn call(process: &mut Process32, parameter: u32, output: u32, size: u32, value: u32) {
    let mut expected = prepare(process, parameter, output, size);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 16);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
}

fn read(process: &Process32, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

#[test]
fn imported_us_key_names_are_nul_terminated_and_bounded_by_capacity() {
    let mut process = process();
    process
        .memory
        .write(u64::from(OUTPUT), &[0xaa; 12])
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();

    call(&mut process, 0x0011_0000, OUTPUT, 12, 1);
    assert_eq!(read(&process, 4), [b'W', 0, 0xaa, 0xaa]);
    call(&mut process, 0x0211_0000, OUTPUT, 12, 1);
    assert_eq!(read(&process, 2), [b'W', 0]);
    call(&mut process, 0x0002_0000, OUTPUT, 12, 1);
    assert_eq!(read(&process, 2), [b'1', 0]);
    call(&mut process, 0x001e_0000, OUTPUT, 12, 1);
    assert_eq!(read(&process, 2), [b'A', 0]);
    call(&mut process, 0x0148_0000, OUTPUT, 12, 2);
    assert_eq!(read(&process, 3), b"Up\0");
    call(&mut process, 0x000e_0000, OUTPUT, 5, 4);
    assert_eq!(read(&process, 6), b"Back\0\xaa");
    call(&mut process, 0x000e_0000, OUTPUT, 1, 0);
    assert_eq!(read(&process, 2), [0, b'a']);
    call(&mut process, 0x0011_0000, 0, 0, 0);
    call(&mut process, 0x0011_0000, 0, u32::MAX, 0);
    assert_eq!(process.last_error().unwrap(), 77);
}

#[test]
fn unmapped_us_scans_return_an_empty_name() {
    let mut process = process();
    for parameter in [
        0,
        0x0055_0000,
        0x0088_0000,
        0x00c8_0000,
        0x00ff_0000,
        0x01c8_0000,
    ] {
        process.memory.write(u64::from(OUTPUT), &[0xaa; 2]).unwrap();
        call(&mut process, parameter, OUTPUT, 2, 0);
        assert_eq!(read(&process, 2), [0, 0xaa]);
    }
}

#[test]
fn fixed_us_names_cover_named_and_character_fallback_keys() {
    let mut process = process();
    for (parameter, name) in [
        (0x0052_0000, &b"Num 0"[..]),
        (0x0048_0000, &b"Num 8"[..]),
        (0x002a_0000, &b"Shift"[..]),
        (0x0036_0000, &b"Right Shift"[..]),
        (0x0236_0000, &b"Shift"[..]),
        (0x011d_0000, &b"Right Ctrl"[..]),
        (0x031d_0000, &b"Ctrl"[..]),
        (0x0138_0000, &b"Right Alt"[..]),
        (0x0338_0000, &b"Alt"[..]),
        (0x0087_0000, &b"F24"[..]),
        (0x000c_0000, &b"-"[..]),
        (0x001a_0000, &b"["[..]),
        (0x0111_0000, &b"W"[..]),
        (0x0101_0000, &b"\x1b"[..]),
        (0x014a_0000, &b"-"[..]),
        (0x017c_0000, &b"\t"[..]),
        (0x015b_0000, &b"Left Windows"[..]),
    ] {
        process
            .memory
            .write(u64::from(OUTPUT), &[0xaa; 16])
            .unwrap();
        call(
            &mut process,
            parameter,
            OUTPUT,
            16,
            u32::try_from(name.len()).unwrap(),
        );
        let mut expected = name.to_vec();
        expected.extend_from_slice(&[0, 0xaa]);
        assert_eq!(read(&process, expected.len()), expected);
    }
}

#[test]
fn faulting_output_preserves_the_call_frame() {
    let mut process = process();
    for output in [0, u32::MAX] {
        let before = prepare(&mut process, 0x00c8_0000, output, 12);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
}
