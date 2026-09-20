#[path = "support/gdi_executable.rs"]
mod gdi_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const GET: u32 = 0x7000_00a0;
const RELEASE: u32 = 0x7000_00a4;
const CAPS: u32 = 0x7000_00a8;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(&gdi_executable::pe32(&[0xcc]), 32).unwrap()
}

fn prepare(process: &mut Process32, api: u32, arguments: &[u32]) -> Cpu32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000).chain(arguments).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + (index * 4) as u64, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn call(process: &mut Process32, api: u32, arguments: &[u32]) -> u32 {
    let mut expected = prepare(process, api, arguments);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from((arguments.len() + 1) * 4).unwrap(),
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
    value
}

#[test]
fn screen_contexts_have_independent_lifetimes_and_consistent_device_properties() {
    let mut process = process();
    let pages = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let first = call(&mut process, GET, &[0]);
    let second = call(&mut process, GET, &[0]);
    assert_ne!(first, 0);
    assert_ne!(first, second);
    for (index, value) in [
        (2, 1),
        (8, 640),
        (10, 480),
        (12, 32),
        (14, 1),
        (24, u32::MAX),
        (88, 96),
        (90, 96),
    ] {
        assert_eq!(call(&mut process, CAPS, &[first, index]), value);
    }
    assert_eq!(call(&mut process, 0x7000_009c, &[0]), 640);
    assert_eq!(call(&mut process, 0x7000_009c, &[1]), 480);
    assert_eq!(call(&mut process, RELEASE, &[0xdead_beef, first]), 1);
    assert_eq!(call(&mut process, RELEASE, &[0, first]), 0);
    assert_eq!(call(&mut process, CAPS, &[first, 8]), 0);
    assert_eq!(call(&mut process, CAPS, &[second, 8]), 640);
    for invalid in [0, first, 1, 0x2000_0000, 0x7000_0800, u32::MAX] {
        assert_eq!(call(&mut process, CAPS, &[invalid, u32::MAX]), 0);
        assert_eq!(call(&mut process, RELEASE, &[0, invalid]), 0);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn live_capacity_is_reclaimed_without_reviving_stale_handles() {
    let mut first = process();
    let mut second = process();
    first
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let mut handles = std::collections::BTreeSet::new();
    for _ in 0..1024 {
        let handle = call(&mut first, GET, &[0]);
        assert_ne!(handle, 0);
        assert!(handles.insert(handle));
    }
    assert_eq!(call(&mut first, GET, &[0]), 0);
    let other = call(&mut second, GET, &[0]);
    assert_eq!(call(&mut first, RELEASE, &[0, other]), 1);
    assert_eq!(call(&mut second, CAPS, &[other, 8]), 640);
    let replacement = call(&mut first, GET, &[0]);
    assert_ne!(replacement, 0);
    assert!(!handles.contains(&replacement));
    assert_eq!(call(&mut first, CAPS, &[other, 8]), 0);
    assert_eq!(call(&mut first, GET, &[0]), 0);
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(first.last_error().unwrap(), 77);
}

#[test]
fn unsupported_inputs_and_bad_frames_do_not_allocate_or_release_contexts() {
    let mut process = process();
    let live = call(&mut process, GET, &[0]);
    for (api, arguments) in [
        (GET, vec![1]),
        (GET, vec![u32::MAX]),
        (CAPS, vec![live, 38]),
        (CAPS, vec![live, u32::MAX]),
    ] {
        let before = prepare(&mut process, api, &arguments);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for (api, arguments) in [
        (GET, vec![0]),
        (RELEASE, vec![0, live]),
        (CAPS, vec![live, 8]),
    ] {
        prepare(&mut process, api, &arguments);
        process
            .memory
            .protect(0x1000_f000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        process
            .memory
            .protect(0x1000_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
    }
    for (api, stack) in [
        (GET, 0x1000_fffc),
        (RELEASE, 0x1000_fff8),
        (CAPS, u32::MAX - 3),
    ] {
        prepare(&mut process, api, &[0, live]);
        process.cpu.set_register(Register32::Esp, stack);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    assert_eq!(call(&mut process, CAPS, &[live, 8]), 640);
    assert_eq!(call(&mut process, GET, &[0]), live + 4);
    assert_eq!(process.last_error().unwrap(), 0);
}

#[test]
fn screen_context_guest_binds_both_dlls_and_matches_single_step_execution() {
    let bytes = gdi_executable::lifecycle();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (19, 5));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..50 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (19, 5));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.cpu.register(Register32::Edx), 1);
    assert_eq!(whole.cpu.register(Register32::Esi), 640);
    assert_eq!(whole.cpu.register(Register32::Edi), 480);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
