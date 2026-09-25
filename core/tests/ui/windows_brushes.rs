use super::brushes_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const GET: u32 = 0x7000_00b0;
const DELETE: u32 = 0x7000_00b8;
const OBJECT: u32 = 0x7000_00b4;
const OUT: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(&brushes_executable::pe32(&[0xcc]), 32).unwrap()
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
fn cached_brushes_share_system_colors_and_survive_delete() {
    let mut process = process();
    let mut other = Process32::load(&brushes_executable::pe32(&[0xcc]), 32).unwrap();
    let pages = process.memory.mapped_pages();
    let mut handles = std::collections::BTreeSet::new();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for index in 0..31 {
        if index == 25 {
            continue;
        }
        let handle = call(&mut process, GET, &[index]);
        assert_ne!(handle, 0);
        assert!(handles.insert(handle));
        assert_eq!(call(&mut process, GET, &[index]), handle);
        assert_eq!(call(&mut other, OBJECT, &[handle, 12, OUT]), 0);
        assert_eq!(call(&mut process, DELETE, &[handle]), 1);
        assert_eq!(call(&mut process, DELETE, &[handle]), 1);
        assert_eq!(call(&mut process, OBJECT, &[handle, 12, OUT]), 12);
        let color = call(&mut process, 0x7000_00ac, &[index]);
        let mut expected = [0; 12];
        expected[4..8].copy_from_slice(&color.to_le_bytes());
        let mut output = [0; 12];
        process.memory.read(u64::from(OUT), &mut output).unwrap();
        assert_eq!(output, expected);
    }
    for index in [31, u32::MAX, 0x8000_0000] {
        assert_eq!(call(&mut process, GET, &[index]), 0);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn brush_descriptor_size_prefix_and_identity_validation_are_exact() {
    let mut process = process();
    let brush = call(&mut process, GET, &[1]);
    let expected = [0, 0, 0, 0, 58, 110, 165, 0, 0, 0, 0, 0];
    for count in 0..=20 {
        process.memory.write(u64::from(OUT), &[0xaa; 24]).unwrap();
        let size = call(&mut process, OBJECT, &[brush, count, OUT + 1]);
        assert_eq!(size, count.min(12));
        let mut actual = [0; 24];
        process.memory.read(u64::from(OUT), &mut actual).unwrap();
        let mut wanted = [0xaa; 24];
        let end = usize::try_from(size).unwrap();
        wanted[1..][..end].copy_from_slice(&expected[..end]);
        assert_eq!(actual, wanted);
    }
    for count in [0, 1, 12, u32::MAX] {
        assert_eq!(call(&mut process, OBJECT, &[brush, count, 0]), 12);
    }
    assert_eq!(call(&mut process, OBJECT, &[brush, 0, u32::MAX]), 0);
    for invalid in [0, brush + 1, brush + 4, 0x7000_0800, u32::MAX] {
        assert_eq!(
            call(&mut process, OBJECT, &[invalid, u32::MAX, u32::MAX]),
            0
        );
        assert_eq!(call(&mut process, DELETE, &[invalid]), 0);
    }
    let dc = call(&mut process, 0x7000_00a0, &[0]);
    for (api, args) in [
        (GET, vec![25]),
        (OBJECT, vec![brush, u32::MAX, OUT]),
        (OBJECT, vec![dc, 12, OUT]),
        (DELETE, vec![dc]),
    ] {
        let before = prepare(&mut process, api, &args);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    assert_eq!(call(&mut process, 0x7000_00a8, &[brush, 8]), 0);
    assert_eq!(call(&mut process, 0x7000_00a4, &[0, brush]), 0);
    assert_eq!(call(&mut process, 0x7000_00a8, &[dc, 8]), 640);
}

#[test]
fn descriptor_writes_preflight_the_complete_prefix_and_obey_output_aliases() {
    let mut process = process();
    let brush = call(&mut process, GET, &[1]);
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_2ff8, &[0xaa; 8]).unwrap();
    process.memory.write(0xffff_fff8, &[0xaa; 8]).unwrap();
    for output in [0x0040_2ff8, 0xffff_fff8, 0x0040_1000] {
        let before = prepare(&mut process, OBJECT, &[brush, 12, output]);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for output in [0x0040_2ff8, 0xffff_fff8] {
        let mut bytes = [0; 8];
        process.memory.read(output, &mut bytes).unwrap();
        assert_eq!(bytes, [0xaa; 8]);
    }
    assert_eq!(call(&mut process, OBJECT, &[brush, 8, 0xffff_fff8]), 8);
    assert_eq!(call(&mut process, OBJECT, &[brush, 12, STACK + 4]), 12);
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, OBJECT, &[brush, 12, 0x7ffd_e034]), 12);
    assert_eq!(process.last_error().unwrap(), 0);
    let mut expected = prepare(&mut process, OBJECT, &[brush, 12, STACK]);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0;
    expected.set_register(Register32::Eax, 12);
    expected.set_register(Register32::Esp, STACK + 16);
    assert_eq!(process.cpu, expected);
}

#[test]
fn frame_faults_do_not_materialize_brushes_or_write_outputs() {
    let mut process = process();
    for (api, args, stack) in [
        (GET, vec![0], 0x1000_fffc),
        (OBJECT, vec![0, 12, OUT], 0x1000_fff4),
        (DELETE, vec![0], u32::MAX - 3),
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
    assert_eq!(call(&mut process, OBJECT, &[0x5000_0000, 0, 0]), 0);
    let brush = call(&mut process, GET, &[0]);
    assert_eq!(call(&mut process, OBJECT, &[brush, 0, 0]), 12);
}

#[test]
fn cached_brush_guest_matches_whole_and_single_step_execution() {
    let bytes = brushes_executable::lifecycle();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 4));
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
    assert_eq!((instructions, calls), (15, 4));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(
        whole.cpu.register(Register32::Eax),
        whole.cpu.register(Register32::Ebx)
    );
    assert_ne!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.cpu.register(Register32::Edx), 0x00c8_d0d4);
    assert_eq!(whole.cpu.register(Register32::Esi), 12);
    assert_eq!(whole.cpu.register(Register32::Edi), 1);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
