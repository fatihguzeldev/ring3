#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0140;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_onexit"]),
        25,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, callback: u32) {
    p.cpu.eip = API;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(STACK) + 4, &callback.to_le_bytes())
        .unwrap();
}

fn call(p: &mut Process32, callback: u32, returned: u32) {
    prepare(p, callback);
    let mut expected = p.cpu;
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, returned);
    expected.set_register(Register32::Esp, STACK + 4);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(p.cpu, expected);
}

#[test]
fn registration_keeps_cdecl_arguments_and_defers_callback_access() {
    let mut p = process();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let pages = p.memory.mapped_pages();
    for callback in [0, 0x0040_1000, 0x0040_1000, 0x7000_0000, u32::MAX] {
        call(&mut p, callback, callback);
        let mut argument = [0; 4];
        p.memory.read(u64::from(STACK) + 4, &mut argument).unwrap();
        assert_eq!(u32::from_le_bytes(argument), callback);
        assert_eq!(p.memory.mapped_pages(), pages);
    }
}

#[test]
fn frame_faults_and_null_do_not_consume_capacity_and_full_registry_returns_null() {
    let mut p = process();
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for stack in [0x1000_fffc, 0x7000_0000, u32::MAX] {
        prepare(&mut p, 0x0040_1000);
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    call(&mut p, 0, 0);
    for callback in 1..=4096 {
        call(&mut p, callback, callback);
    }
    call(&mut p, 0x0040_1000, 0);
    call(&mut p, 0, 0);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 123);
    assert_eq!(p.last_error().unwrap(), 77);
    call(&mut process(), 0x0040_1000, 0x0040_1000);
}
