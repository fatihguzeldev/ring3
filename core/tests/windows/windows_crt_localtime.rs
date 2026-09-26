use super::imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const IAT: u32 = 0x0040_2060;
const LOCALTIME: u32 = 0x7000_01f8;
const STACK: u32 = 0x1000_ff00;
const INPUT: u32 = 0x0040_2280;
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
    let bytes = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["localtime"]);
    let process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(read(&process, IAT), LOCALTIME);
    process
}

fn prepare(process: &mut Process32, input: u32) {
    process.cpu.eip = read(process, IAT);
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 0x5555_5555);
    write(process, STACK, RETURN);
    write(process, STACK + 4, input);
}

fn call(process: &mut Process32, input: u32, expected: u32) {
    prepare(process, input);
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
    assert_eq!(read(process, STACK + 4), input);
}

fn fields(process: &Process32, address: u32) -> [u32; 9] {
    std::array::from_fn(|index| {
        read(
            process,
            address + u32::try_from(index).expect("nine fields") * 4,
        )
    })
}

#[test]
fn crt_localtime_converts_utc_calendar_to_stable_thread_storage() {
    let mut process = load();
    let output = process.cpu.fs_base() + 0x100;
    for (seconds, expected) in [
        (0, [0, 0, 0, 1, 0, 70, 4, 0, 0]),
        (314, [14, 5, 0, 1, 0, 70, 4, 0, 0]),
        (951_827_696, [56, 34, 12, 29, 1, 100, 2, 59, 0]),
        (1_709_254_923, [3, 2, 1, 1, 2, 124, 5, 60, 0]),
        (2_147_483_647, [7, 14, 3, 19, 0, 138, 2, 18, 0]),
    ] {
        write(&mut process, INPUT, seconds);
        call(&mut process, INPUT, output);
        assert_eq!(fields(&process, output), expected);
    }
    write(&mut process, INPUT, u32::MAX);
    call(&mut process, INPUT, 0);
    assert_eq!(fields(&process, output), [7, 14, 3, 19, 0, 138, 2, 18, 0]);
}

#[test]
fn crt_localtime_bad_source_faults_without_state_change() {
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
}
