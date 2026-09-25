use super::cursor_position_executable;
use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const GET: u32 = 0x7000_00cc;
const SET: u32 = 0x7000_00d0;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "USER32.dll", &["GetCursorPos", "SetCursorPos"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + (index * 4) as u64, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn call(process: &mut Process32, api: u32, args: &[u32], result: u32) {
    let mut expected = prepare(process, api, args);
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from((args.len() + 1) * 4).unwrap(),
    );
    expected.set_register(Register32::Eax, result);
    assert_eq!(process.cpu, expected);
}

fn read_point(process: &Process32, address: u32) -> [u32; 2] {
    let mut bytes = [0; 8];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    [
        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        u32::from_le_bytes(bytes[4..].try_into().unwrap()),
    ]
}

#[test]
fn guest_position_roundtrips_clamps_signed_coordinates_and_preserves_selection() {
    let mut first = process();
    call(&mut first, GET, &[OUTPUT], 1);
    assert_eq!(read_point(&first, OUTPUT), [0, 0]);
    call(&mut first, 0x7000_00bc, &[0, 32512], 0x4000_0000);
    call(&mut first, 0x7000_00c0, &[0x4000_0000], 0);
    first
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (input, expected) in [
        ([0, 0], [0, 0]),
        ([320, 240], [320, 240]),
        ([639, 479], [639, 479]),
        ([640, 480], [639, 479]),
        ([-1, -1], [0, 0]),
        ([i32::MIN, i32::MAX], [0, 479]),
        ([i32::MAX, i32::MIN], [639, 0]),
    ] {
        call(&mut first, SET, &input.map(i32::cast_unsigned), 1);
        call(&mut first, GET, &[OUTPUT], 1);
        assert_eq!(read_point(&first, OUTPUT), expected);
        call(&mut first, 0x7000_00c4, &[], 0x4000_0000);
    }
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(first.last_error().unwrap(), 77);
    let mut second = process();
    call(&mut second, GET, &[OUTPUT], 1);
    assert_eq!(read_point(&second, OUTPUT), [0, 0]);
    call(&mut first, GET, &[OUTPUT], 1);
    assert_eq!(read_point(&first, OUTPUT), [639, 0]);
}

#[test]
fn point_outputs_touch_eight_bytes_and_allow_unaligned_boundaries_and_aliases() {
    let mut process = process();
    call(&mut process, SET, &[123, 234], 1);
    for page in [0x5000, 0x0040_3000, 0xffff_f000] {
        process
            .memory
            .map_zeroed(page, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
    }
    for pointer in [OUTPUT + 1, 0x0040_2ffc, 0xffff_fff8, STACK + 4, 0x7ffd_e034] {
        call(&mut process, GET, &[pointer], 1);
        assert_eq!(read_point(&process, pointer), [123, 234]);
    }
    assert_eq!(process.last_error().unwrap(), 123);
    process.memory.write(0x5080, &[0xaa; 10]).unwrap();
    process
        .memory
        .protect(
            0x5000,
            PAGE_SIZE,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    call(&mut process, GET, &[0x5081], 1);
    process
        .memory
        .protect(0x5000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let mut bytes = [0; 10];
    process.memory.read(0x5080, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa, 123, 0, 0, 0, 234, 0, 0, 0, 0xaa]);
    let mut expected = prepare(&mut process, GET, &[STACK]);
    expected.eip = 123;
    expected.set_register(Register32::Esp, STACK + 8);
    expected.set_register(Register32::Eax, 1);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu, expected);
    assert_eq!(read_point(&process, STACK), [123, 234]);
}

#[test]
fn output_and_frame_faults_preserve_position_cpu_and_output_prefix() {
    let mut process = process();
    call(&mut process, SET, &[123, 234], 1);
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_2ffc, &[0xaa; 4]).unwrap();
    process.memory.write(0xffff_fffc, &[0xbb; 4]).unwrap();
    for pointer in [0x0040_2ffc, 0x0040_3000, 0xffff_fffc] {
        let before = prepare(&mut process, GET, &[pointer]);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for (address, expected) in [(0x0040_2ffc, [0xaa; 4]), (0xffff_fffc, [0xbb; 4])] {
        let mut bytes = [0; 4];
        process.memory.read(address, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, GET, &[OUTPUT]);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    for (api, args) in [(GET, vec![OUTPUT]), (SET, vec![500, 400])] {
        prepare(&mut process, api, &args);
        process.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
    }
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut process, GET, &[OUTPUT], 1);
    assert_eq!(read_point(&process, OUTPUT), [123, 234]);
}

#[test]
fn null_point_returns_noaccess_and_protected_error_storage_is_atomic() {
    let mut process = process();
    call(&mut process, SET, &[123, 234], 1);
    call(&mut process, GET, &[0], 0);
    assert_eq!(process.last_error().unwrap(), 998);
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, GET, &[0]);
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.last_error().unwrap(), 77);
    call(&mut process, GET, &[OUTPUT], 1);
    assert_eq!(read_point(&process, OUTPUT), [123, 234]);
}

#[test]
fn cursor_position_guest_matches_whole_and_single_instruction_execution() {
    let bytes = cursor_position_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..30 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (8, 2));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(whole.cpu.register(Register32::Eax), 1);
    assert_eq!(whole.cpu.register(Register32::Ecx), 210);
    assert_eq!(whole.cpu.register(Register32::Edx), 120);
    assert_eq!(read_point(&whole, OUTPUT), [210, 120]);
    assert_eq!(read_point(&stepped, OUTPUT), [210, 120]);
}
