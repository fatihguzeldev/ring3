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
fn unmodeled_codes_and_faulting_output_preserve_the_call_frame() {
    let mut process = process();
    for parameter in [0, 0x00ff_0000, 0x0111_0000] {
        let before = prepare(&mut process, parameter, OUTPUT, 12);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for output in [0, u32::MAX] {
        let before = prepare(&mut process, 0x0011_0000, output, 12);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
}
