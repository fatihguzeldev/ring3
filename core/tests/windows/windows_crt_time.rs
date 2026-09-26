use super::imported_executable;

use std::time::Duration;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const IAT: u32 = 0x0040_2060;
const TIME: u32 = 0x7000_01f4;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2280;
const RETURN: u32 = 0x0040_1000;

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn write(process: &mut Process32, address: u32, value: u32) {
    process
        .memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn load() -> Process32 {
    let bytes = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["time"]);
    let process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(read(&process, IAT), TIME);
    process
}

fn prepare(process: &mut Process32, output: u32) {
    process.cpu.eip = read(process, IAT);
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 0x5555_5555);
    write(process, STACK, RETURN);
    write(process, STACK + 4, output);
}

#[test]
fn crt_time_returns_virtual_seconds_and_writes_optional_time32_output() {
    let mut process = load();
    for (nanos, output, expected) in [
        (0, OUTPUT, 0),
        (5_999_999_999, OUTPUT, 5),
        (6_000_000_000, 0, 6),
        (2_147_483_648_000_000_000, OUTPUT, u32::MAX),
    ] {
        process
            .set_elapsed_time(Duration::from_nanos(nanos))
            .unwrap();
        write(&mut process, OUTPUT, 0x5555_5555);
        prepare(&mut process, output);
        let mut cpu = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        cpu.eip = RETURN;
        cpu.set_register(Register32::Esp, STACK + 4);
        cpu.set_register(Register32::Eax, expected);
        assert_eq!(process.cpu, cpu);
        assert_eq!(read(&process, STACK + 4), output);
        assert_eq!(
            read(&process, OUTPUT),
            if output == 0 { 0x5555_5555 } else { expected }
        );
    }
}

#[test]
fn crt_time_bad_output_faults_without_cpu_or_memory_change() {
    let mut process = load();
    prepare(&mut process, u32::MAX - 1);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(read(&process, STACK + 4), u32::MAX - 1);
}
