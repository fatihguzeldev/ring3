#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const INIT: u32 = 0x7000_0034;
const ENTER: u32 = 0x7000_0038;
const TRY: u32 = 0x7000_003c;
const LEAVE: u32 = 0x7000_0060;
const DELETE: u32 = 0x7000_0064;
const OBJECT: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "kernel32.dll",
            &[
                "InitializeCriticalSection",
                "EnterCriticalSection",
                "TryEnterCriticalSection",
                "DeleteCriticalSection",
            ],
        ),
        32,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32, pointer: u32) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(STACK) + 4, &pointer.to_le_bytes())
        .unwrap();
}

fn call(p: &mut Process32, api: u32, pointer: u32) {
    prepare(p, api, pointer);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x0040_1000);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 8);
}

fn words(p: &Process32, pointer: u32) -> [u32; 6] {
    let mut bytes = [0; 24];
    p.memory.read(u64::from(pointer), &mut bytes).unwrap();
    std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}

#[test]
fn recursive_ownership_balances_and_deleted_objects_can_be_reinitialized() {
    let mut p = process();
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    p.cpu.set_register(Register32::Eax, 42);
    p.cpu.eflags = 0xad7;
    call(&mut p, INIT, OBJECT);
    assert_eq!(words(&p, OBJECT), [0, u32::MAX, 0, 0, 0, 0]);
    assert_eq!(p.cpu.register(Register32::Eax), 42);
    call(&mut p, ENTER, OBJECT);
    call(&mut p, TRY, OBJECT);
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(words(&p, OBJECT), [0, 1, 2, 1, 0, 0]);
    call(&mut p, INIT, OBJECT + 24);
    assert_eq!(words(&p, OBJECT + 24), [0, u32::MAX, 0, 0, 0, 0]);
    call(&mut p, LEAVE, OBJECT);
    assert_eq!(words(&p, OBJECT), [0, 0, 1, 1, 0, 0]);
    call(&mut p, LEAVE, OBJECT);
    assert_eq!(words(&p, OBJECT), [0, u32::MAX, 0, 0, 0, 0]);
    call(&mut p, DELETE, OBJECT);
    assert_eq!(words(&p, OBJECT), [0; 6]);
    call(&mut p, INIT, OBJECT);
    call(&mut p, DELETE, OBJECT);
    assert_eq!(p.last_error().unwrap(), 99);
    assert_eq!(p.cpu.eflags, 0xad7);
}

#[test]
fn invalid_lifecycle_and_modified_objects_stop_without_mutation() {
    let mut p = process();
    call(&mut p, INIT, OBJECT);
    for (api, pointer) in [
        (INIT, OBJECT),
        (INIT, OBJECT + 4),
        (INIT, OBJECT - 4),
        (LEAVE, OBJECT),
        (ENTER, OBJECT + 24),
        (DELETE, OBJECT + 24),
    ] {
        prepare(&mut p, api, pointer);
        let before = p.cpu;
        let state = words(&p, OBJECT);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: api }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(words(&p, OBJECT), state);
    }
    call(&mut p, ENTER, OBJECT);
    prepare(&mut p, DELETE, OBJECT);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: DELETE }
    );
    assert_eq!(words(&p, OBJECT)[2], 1);
    p.memory
        .write(u64::from(OBJECT + 12), &2_u32.to_le_bytes())
        .unwrap();
    prepare(&mut p, TRY, OBJECT);
    let before = p.cpu;
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: TRY }
    );
    assert_eq!(p.cpu, before);
    let mut separate = process();
    prepare(&mut separate, ENTER, OBJECT);
    assert_eq!(
        separate.run(1).reason,
        ProcessStop::UnsupportedApi { address: ENTER }
    );
}

#[test]
fn failed_initialization_and_release_can_be_retried_without_state_loss() {
    let mut p = process();
    let pointer = 0x0040_2ff0;
    p.memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    p.memory.write(u64::from(pointer), &[0xab; 16]).unwrap();
    for address in [pointer, u32::MAX - 15] {
        prepare(&mut p, INIT, address);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(result.api_calls, 0);
        assert_eq!(p.cpu, before);
    }
    let mut bytes = [0; 16];
    p.memory.read(u64::from(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0xab; 16]);
    p.memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, INIT, pointer);
    call(&mut p, ENTER, pointer);
    p.memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    prepare(&mut p, LEAVE, pointer);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(words(&p, pointer)[2], 1);
    p.memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, LEAVE, pointer);
    call(&mut p, DELETE, pointer);
}

#[test]
fn return_frame_faults_and_output_aliases_follow_normal_dispatch() {
    let mut p = process();
    prepare(&mut p, INIT, OBJECT);
    p.cpu.set_register(Register32::Esp, u32::MAX - 3);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    call(&mut p, INIT, OBJECT);
    prepare(&mut p, INIT, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 8);
    assert_eq!(words(&p, STACK), [0, u32::MAX, 0, 0, 0, 0]);
    call(&mut p, INIT, 0x7ffd_e034);
    assert_eq!(p.last_error().unwrap(), 0);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, INIT, 0xffff_ffe8);
    call(&mut p, ENTER, 0xffff_ffe8);
    assert_eq!(words(&p, 0xffff_ffe8), [0, 0, 1, 1, 0, 0]);
}
