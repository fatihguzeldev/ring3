use super::imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const SET: u32 = 0x7000_013c;
const STACK: u32 = 0x1000_ff00;
const TEXT: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_setmbcp", "_mbsrchr", "_mbsinc"]),
        25,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    for (index, value) in [0x0040_1000].iter().chain(args).enumerate() {
        p.memory
            .write(u64::from(STACK) + (index * 4) as u64, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(p: &mut Process32, api: u32, args: &[u32], expected: u32) {
    prepare(p, api, args);
    let mut cpu = p.cpu;
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, expected);
    cpu.set_register(Register32::Esp, STACK + 4);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(p.cpu, cpu);
}

#[test]
fn accepted_selectors_keep_single_byte_search_and_traversal_without_error_access() {
    let mut p = process();
    p.memory
        .write(u64::from(TEXT), &[0x81, 0xfe, 0x81, 0])
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for selector in [0xffff_fffd, 1252, 0xffff_fffe, 437, 0, 0xffff_fffd] {
        call(&mut p, SET, &[selector], 0);
        call(&mut p, 0x7000_0088, &[], 1252);
        call(&mut p, 0x7000_008c, &[], 437);
        call(&mut p, 0x7000_012c, &[TEXT, 0x181], TEXT + 2);
        call(&mut p, 0x7000_012c, &[TEXT, 0], TEXT + 3);
        call(&mut p, 0x7000_0130, &[TEXT], TEXT + 1);
        call(&mut p, 0x7000_0130, &[TEXT + 3], TEXT + 4);
    }
    let mut bytes = [0; 4];
    p.memory.read(u64::from(TEXT), &mut bytes).unwrap();
    assert_eq!(bytes, [0x81, 0xfe, 0x81, 0]);
}

#[test]
fn unsupported_encodings_preserve_cpu_errors_and_api_budget() {
    let mut p = process();
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    call(&mut p, SET, &[437], 0);
    for selector in [932, 65001, 20127, 1, u32::MAX, 0xffff_fffc, 0x8000_0000] {
        prepare(&mut p, SET, &[selector]);
        let before = p.cpu;
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: SET });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 123);
    assert_eq!(p.last_error().unwrap(), 77);
    call(&mut p, SET, &[0], 0);
}

#[test]
fn complete_cdecl_frame_is_required_before_selection() {
    let mut p = process();
    for stack in [0x1000_fffc, 0x7000_0000, u32::MAX] {
        prepare(&mut p, SET, &[1252]);
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
    call(&mut p, SET, &[1252], 0);
}
