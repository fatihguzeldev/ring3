use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const LOAD: u32 = 0x7000_00bc;
const SET: u32 = 0x7000_00c0;
const GET: u32 = 0x7000_00c4;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "USER32.dll",
            &["LoadCursorA", "SetCursor", "GetCursor"],
        ),
        32,
    )
    .unwrap()
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
fn predefined_cursor_cache_and_selection_are_process_local() {
    let mut process = process();
    let mut other = Process32::load(
        &imported_executable::pe32(&[0xcc], "USER32.dll", &["SetCursor"]),
        32,
    )
    .unwrap();
    let pages = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let mut handles = std::collections::BTreeSet::new();
    assert_eq!(call(&mut process, GET, &[]), 0);
    for id in [
        32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650,
        32651,
    ] {
        let handle = call(&mut process, LOAD, &[0, id]);
        assert_ne!(handle, 0);
        assert!(handles.insert(handle));
        assert_eq!(call(&mut process, LOAD, &[0, id]), handle);
        assert_eq!(call(&mut process, GET, &[]), 0);
        assert_eq!(call(&mut process, SET, &[handle]), 0);
        assert_eq!(call(&mut process, SET, &[handle]), handle);
        assert_eq!(call(&mut process, GET, &[]), handle);
        assert_eq!(call(&mut other, SET, &[handle]), 0);
        assert_eq!(other.last_error().unwrap(), 1402);
        assert_eq!(call(&mut other, GET, &[]), 0);
        assert_eq!(call(&mut process, SET, &[0]), handle);
        assert_eq!(call(&mut process, GET, &[]), 0);
    }
    let arrow = call(&mut process, LOAD, &[0, 32512]);
    let wait = call(&mut process, LOAD, &[0, 32514]);
    assert_eq!(call(&mut process, SET, &[arrow]), 0);
    assert_eq!(call(&mut process, SET, &[wait]), arrow);
    assert_eq!(call(&mut process, GET, &[]), wait);
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn unsupported_loads_and_invalid_selections_preserve_current_cursor() {
    let mut process = process();
    let wait = call(&mut process, LOAD, &[0, 32514]);
    call(&mut process, SET, &[wait]);
    for args in [
        [1, 32512],
        [0, 0],
        [0, 32517],
        [0, 32640],
        [0, 32641],
        [0, 32671],
        [0, 0x0040_2180],
        [0, u32::MAX],
    ] {
        let before = prepare(&mut process, LOAD, &args);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: LOAD });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for handle in [1, wait + 1, wait + 4, 0x5000_0000, 0x6000_0000, u32::MAX] {
        assert_eq!(call(&mut process, SET, &[handle]), 0);
        assert_eq!(process.last_error().unwrap(), 1402);
        assert_eq!(call(&mut process, GET, &[]), wait);
    }
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, SET, &[1]);
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(call(&mut process, GET, &[]), wait);
    assert_eq!(process.last_error().unwrap(), 77);
}

#[test]
fn frame_faults_preserve_cache_and_selection_and_error_output_can_alias_return() {
    let mut process = process();
    let wait = call(&mut process, LOAD, &[0, 32514]);
    call(&mut process, SET, &[wait]);
    for (api, args, stack) in [
        (LOAD, vec![0, 32512], 0x1000_fff8),
        (SET, vec![0], 0x1000_fffc),
        (GET, vec![], 0),
    ] {
        prepare(&mut process, api, &args);
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
    assert_eq!(call(&mut process, SET, &[0x4000_0000]), 0);
    assert_eq!(process.last_error().unwrap(), 1402);
    assert_eq!(call(&mut process, GET, &[]), wait);
    prepare(&mut process, SET, &[1]);
    process.cpu.set_register(Register32::Esp, 0x7ffd_e034);
    process
        .memory
        .write(0x7ffd_e034, &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(0x7ffd_e038, &1_u32.to_le_bytes())
        .unwrap();
    let mut expected = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 1402;
    expected.set_register(Register32::Esp, 0x7ffd_e03c);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(process.cpu, expected);
    assert_eq!(call(&mut process, GET, &[]), wait);
}

use super::cursors_executable;

#[test]
fn cursor_guest_matches_whole_and_single_step_execution() {
    let bytes = cursors_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (13, 5));
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
    assert_eq!((instructions, calls), (13, 5));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_ne!(whole.cpu.register(Register32::Ebx), 0);
    assert_eq!(
        whole.cpu.register(Register32::Ebx),
        whole.cpu.register(Register32::Esi)
    );
    assert_eq!(
        whole.cpu.register(Register32::Ebx),
        whole.cpu.register(Register32::Edi)
    );
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
