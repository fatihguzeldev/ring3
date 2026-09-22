#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01b4;
const STACK: u32 = 0x1000_ff00;
const PAGE: u32 = 0x5000_0000;
const OBJECT: u32 = PAGE;
const VTABLE: u32 = PAGE + 0x100;
const LOCATOR: u32 = PAGE + 0x200;
const HIERARCHY: u32 = PAGE + 0x300;
const ARRAY: u32 = PAGE + 0x400;
const TARGET_BASE: u32 = PAGE + 0x500;
const SOURCE_BASE: u32 = PAGE + 0x520;
const TARGET_TYPE: u32 = PAGE + 0x600;
const SOURCE_TYPE: u32 = PAGE + 0x640;
const OTHER_TYPE: u32 = PAGE + 0x680;

fn put(process: &mut Process32, address: u32, words: &[u32]) {
    for (index, word) in words.iter().enumerate() {
        process
            .memory
            .write(u64::from(address) + (index * 4) as u64, &word.to_le_bytes())
            .unwrap();
    }
}

fn process() -> Process32 {
    let image = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["__RTDynamicCast"]);
    let mut process = Process32::load(&image, 32).unwrap();
    process
        .memory
        .map_zeroed(u64::from(PAGE), 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut process, OBJECT, &[VTABLE]);
    put(&mut process, VTABLE - 4, &[LOCATOR]);
    put(&mut process, LOCATOR, &[0, 0, 0, TARGET_TYPE, HIERARCHY]);
    put(&mut process, HIERARCHY, &[0, 0, 2, ARRAY]);
    put(&mut process, ARRAY, &[TARGET_BASE, SOURCE_BASE]);
    put(
        &mut process,
        TARGET_BASE,
        &[TARGET_TYPE, 1, 0, u32::MAX, 0, 0],
    );
    put(
        &mut process,
        SOURCE_BASE,
        &[SOURCE_TYPE, 0, 0, u32::MAX, 0, 0],
    );
    for (address, name) in [
        (TARGET_TYPE, b".?AVTarget@@\0".as_slice()),
        (SOURCE_TYPE, b".?AVSource@@\0".as_slice()),
        (OTHER_TYPE, b".?AVOther@@\0".as_slice()),
    ] {
        process.memory.write(u64::from(address + 8), name).unwrap();
    }
    process
}

fn prepare(process: &mut Process32, object: u32, target: u32, is_reference: u32) {
    put(
        process,
        STACK,
        &[0x0040_1000, object, 0, SOURCE_TYPE, target, is_reference],
    );
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
}

#[test]
fn nonvirtual_rtti_casts_to_a_real_subobject_and_missing_type_returns_null() {
    let mut process = process();
    prepare(&mut process, OBJECT, TARGET_TYPE, 0);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Eax), OBJECT);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 4);

    prepare(&mut process, OBJECT, OTHER_TYPE, 0);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
}

#[test]
fn rtti_offsets_adjust_from_source_subobject_to_target_subobject() {
    let mut process = process();
    let source_object = OBJECT + 4;
    put(&mut process, source_object, &[VTABLE]);
    put(&mut process, LOCATOR + 4, &[4]);
    put(&mut process, TARGET_BASE + 8, &[8]);
    put(&mut process, SOURCE_BASE + 8, &[4]);
    prepare(&mut process, source_object, TARGET_TYPE, 0);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), OBJECT + 8);
}

#[test]
fn malformed_rtti_faults_without_completing_the_api_call() {
    let mut process = process();
    put(&mut process, HIERARCHY + 12, &[0x6000_0000]);
    prepare(&mut process, OBJECT, TARGET_TYPE, 0);
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
fn reference_failure_and_unsupported_hierarchy_do_not_claim_a_cast() {
    let mut process = process();
    prepare(&mut process, OBJECT, OTHER_TYPE, 1);
    let before = process.cpu;
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!(result.api_calls, 0);
    assert_eq!(process.cpu, before);

    put(&mut process, HIERARCHY + 8, &[65]);
    prepare(&mut process, OBJECT, TARGET_TYPE, 0);
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!(result.api_calls, 0);
}
