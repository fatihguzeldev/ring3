#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const HANDLE: u32 = 0x7000_00d8;
const ID: u32 = 0x7000_00dc;
const STACK: u32 = 0x1000_fff0;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "KERNEL32.dll",
            &["GetCurrentThread", "GetCurrentThreadId"],
        ),
        25,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
}

fn call(p: &mut Process32, api: u32) -> u32 {
    prepare(p, api);
    let mut expected = p.cpu;
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(Register32::Esp, STACK + 4);
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn current_identity_agrees_with_initial_teb_and_critical_section_owner() {
    let mut p = process();
    p.cpu.eflags = 0xcd7;
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    let mut before = [0; 4096];
    p.memory.read(0x7ffd_e000, &mut before).unwrap();
    assert_eq!(&before[0x24..0x28], &1_u32.to_le_bytes());
    let pages = p.memory.mapped_pages();
    for _ in 0..3 {
        assert_eq!(call(&mut p, HANDLE), u32::MAX - 1);
        assert_eq!(call(&mut p, ID), 1);
    }
    let mut after = [0; 4096];
    p.memory.read(0x7ffd_e000, &mut after).unwrap();
    assert_eq!(before, after);
    assert_eq!(p.last_error().unwrap(), 99);
    assert_eq!(p.memory.mapped_pages(), pages);
    for api in [0x7000_0034, 0x7000_0038] {
        prepare(&mut p, api);
        p.memory
            .write(u64::from(STACK + 4), &0x0040_2180_u32.to_le_bytes())
            .unwrap();
        assert_eq!(p.run(1).api_calls, 1);
    }
    let mut owner = [0; 4];
    p.memory.read(0x0040_218c, &mut owner).unwrap();
    assert_eq!(u32::from_le_bytes(owner), call(&mut p, ID));
}

#[test]
fn id_reads_the_guest_teb_while_pseudo_handle_needs_no_teb_access() {
    let mut p = process();
    p.memory.write(0x7ffd_e024, &77_u32.to_le_bytes()).unwrap();
    assert_eq!(call(&mut p, ID), 77);
    let mut other = process();
    assert_eq!(call(&mut other, ID), 1);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, ID), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, HANDLE), u32::MAX - 1);
    prepare(&mut p, ID);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn invalid_frames_preserve_identity_calls_and_boundary_returns_are_supported() {
    let mut p = process();
    for api in [HANDLE, ID] {
        for stack in [0x1001_0000, u32::MAX - 2] {
            prepare(&mut p, api);
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
        for stack in [STACK + 1, 0x1000_fffc] {
            p.cpu.eip = api;
            p.cpu.set_register(Register32::Esp, stack);
            p.memory
                .write(u64::from(stack), &0x0040_1000_u32.to_le_bytes())
                .unwrap();
            assert_eq!(p.run(1).api_calls, 1);
            assert_eq!(p.cpu.register(Register32::Esp), stack + 4);
        }
    }
}

#[path = "support/thread_identity_executable.rs"]
mod thread_identity_executable;

#[test]
fn thread_identity_guest_matches_whole_and_single_instruction_execution() {
    let bytes = thread_identity_executable::pe32();
    let mut whole = Process32::load(&bytes, 25).unwrap();
    let mut stepped = Process32::load(&bytes, 25).unwrap();
    let result = whole.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 3));
    let (mut instructions, mut apis) = (0, 0);
    loop {
        let result = stepped.run(1);
        instructions += result.instructions;
        apis += result.api_calls;
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!((instructions, apis), (7, 3));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 1);
    assert_eq!(whole.cpu.register(Register32::Ebx), u32::MAX - 1);
    assert_eq!(whole.cpu.register(Register32::Ecx), 1);
    assert_eq!(whole.cpu.register(Register32::Edx), 1);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
