#[path = "support/current_directory_executable.rs"]
mod current_directory_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_query_sizes_and_copies_the_default_root() {
    let mut p = Process32::load(&current_directory_executable::pe32(), 32).unwrap();
    let result = p.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
    assert_eq!(p.cpu.register(Register32::Eax), 3);
    assert_eq!(p.cpu.register(Register32::Ebx), 4);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 4];
    p.memory.read(0x0040_2280, &mut bytes).unwrap();
    assert_eq!(&bytes, b"C:\\\0");
}

const API: u32 = 0x7000_0228;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2280;

use ring3_core::execution::{Cpu32, LoadError, PAGE_SIZE, Permissions, ProcessOptions};

fn load(path: &[u8]) -> Process32 {
    Process32::load_with_options(
        &current_directory_executable::pe32(),
        64,
        ProcessOptions {
            current_directory: path,
            image_path: b"D:\\App\\demo.exe",
            command_line: b"elsewhere.exe",
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, capacity: u32, output: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000_u32, capacity, output].iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn completed(p: &mut Process32, mut before: Cpu32, value: u32, target: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    before.set_register(Register32::Eax, value);
    before.set_register(Register32::Esp, STACK + 12);
    before.eip = target;
    assert_eq!(p.cpu, before);
}

fn fault(p: &mut Process32, before: Cpu32) {
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn directory_is_owned_isolated_and_independent_of_other_process_inputs() {
    let mut path = b"E:\\Data\\Levels".to_vec();
    let mut p = load(&path);
    path.fill(b'?');
    drop(path);
    let mut other = load(b"z:\\");
    for (p, expected) in [
        (&mut p, &b"E:\\Data\\Levels\0"[..]),
        (&mut other, &b"z:\\\0"[..]),
    ] {
        let before = prepare(p, 64, OUTPUT);
        completed(
            p,
            before,
            u32::try_from(expected.len() - 1).unwrap(),
            0x0040_1000,
        );
        let mut bytes = vec![0; expected.len()];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
}

#[test]
fn path_admission_rejects_unsupported_forms_and_bounds_owned_input() {
    for path in [
        &b""[..],
        b"C:",
        b"C:relative",
        b"\\root",
        b"\\\\host\\share",
        b"C:/path",
        b"1:\\",
        b"C:\\path\\",
        b"C:\\a\\\\b",
        b"C:\\.\\a",
        b"C:\\a\\..",
        b"C:\\a\0b",
        b"C:\\a*b",
        b"C:\\a\xff",
    ] {
        assert!(
            matches!(
                Process32::load_with_options(
                    &current_directory_executable::pe32(),
                    64,
                    ProcessOptions {
                        current_directory: path,
                        ..ProcessOptions::default()
                    }
                ),
                Err(LoadError::InvalidProcessParameters)
            ),
            "{path:?}"
        );
    }
    let mut path = vec![b'a'; 32767];
    path[..3].copy_from_slice(b"C:\\");
    let mut p = load(&path);
    p.memory
        .map_zeroed(0x3000_0000, 8 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut p, 32768, 0x3000_0000);
    completed(&mut p, before, 32767, 0x0040_1000);
    let mut bytes = vec![0; 32768];
    p.memory.read(0x3000_0000, &mut bytes).unwrap();
    assert_eq!(&bytes[..32767], path);
    assert_eq!(bytes[32767], 0);
    path.push(b'a');
    assert!(matches!(
        Process32::load_with_options(
            &current_directory_executable::pe32(),
            64,
            ProcessOptions {
                current_directory: &path,
                ..ProcessOptions::default()
            }
        ),
        Err(LoadError::InvalidProcessParameters)
    ));
}

#[test]
fn sizing_and_exact_copy_preserve_last_error_and_ignore_excess_capacity() {
    let mut p = load(b"Q:\\Data");
    for capacity in [0, 1, 7, 8, 9, u32::MAX] {
        p.memory.write(u64::from(OUTPUT), &[0x55; 12]).unwrap();
        p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
        let before = prepare(&mut p, capacity, OUTPUT);
        completed(
            &mut p,
            before,
            if capacity < 8 { 8 } else { 7 },
            0x0040_1000,
        );
        let mut bytes = [0; 12];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        let mut expected = [0x55; 12];
        if capacity >= 8 {
            expected[..8].copy_from_slice(b"Q:\\Data\0");
        }
        assert_eq!(bytes, expected);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for output in [0, u32::MAX, 0x7000_0000] {
        let before = prepare(&mut p, 7, output);
        completed(&mut p, before, 8, 0x0040_1000);
    }
    let before = prepare(&mut p, u32::MAX, 0x0040_2ff8);
    completed(&mut p, before, 7, 0x0040_1000);
}

#[test]
fn checked_copy_handles_write_only_cross_page_and_top_of_guest32() {
    let mut p = load(b"Q:\\Data");
    p.memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for output in [OUTPUT + 1, 0x0040_2ffd, u32::MAX - 7] {
        let before = prepare(&mut p, 8, output);
        completed(&mut p, before, 7, 0x0040_1000);
        let mut bytes = [0; 8];
        p.memory.read(u64::from(output), &mut bytes).unwrap();
        assert_eq!(&bytes, b"Q:\\Data\0");
    }
    p.memory
        .protect(
            0xffff_f000,
            PAGE_SIZE,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    let before = prepare(&mut p, 8, u32::MAX - 7);
    completed(&mut p, before, 7, 0x0040_1000);
    let before = prepare(&mut p, 8, u32::MAX - 6);
    fault(&mut p, before);
}

#[test]
fn faulting_spans_and_frames_leave_cpu_and_outputs_unchanged() {
    let mut p = load(b"Q:\\Data");
    p.memory.write(0x0040_2ffc, &[0x55; 4]).unwrap();
    for output in [0, 0x0040_1000, 0x0040_2ffc, u32::MAX] {
        let before = prepare(&mut p, 8, output);
        fault(&mut p, before);
        let mut tail = [0; 4];
        p.memory.read(0x0040_2ffc, &mut tail).unwrap();
        assert_eq!(tail, [0x55; 4]);
    }
    p.memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
    prepare(&mut p, 0, OUTPUT);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    fault(&mut p, before);
    let mut bytes = [0; 8];
    p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    assert_eq!(bytes, [0x55; 8]);
}

#[test]
fn aliases_use_captured_arguments_and_zero_budget_does_no_work() {
    let mut p = load(b"Q:\\Data");
    for output in [STACK, STACK + 4, STACK + 8, 0x7ffd_e034] {
        let before = prepare(&mut p, 8, output);
        completed(
            &mut p,
            before,
            7,
            if output == STACK {
                u32::from_le_bytes(*b"Q:\\D")
            } else {
                0x0040_1000
            },
        );
        let mut bytes = [0; 8];
        p.memory.read(u64::from(output), &mut bytes).unwrap();
        assert_eq!(&bytes, b"Q:\\Data\0");
    }
    let before = prepare(&mut p, 8, 0);
    let result = p.run(0);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn configured_import_query_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = load(b"Q:\\Data");
        let (mut instructions, mut calls) = (0, 0);
        loop {
            let result = p.run(budget);
            instructions += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(instructions + calls < 50);
        }
        assert_eq!((instructions, calls), (8, 2));
        assert_eq!(p.cpu.register(Register32::Eax), 7);
        assert_eq!(p.cpu.register(Register32::Ebx), 8);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 8];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(&bytes, b"Q:\\Data\0");
    }
}
