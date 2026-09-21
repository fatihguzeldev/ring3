#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00e0;
const OUTPUT: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn process(options: ProcessOptions<'_>) -> Process32 {
    Process32::load_with_options(
        &imported_executable::pe32(&[0xcc], "kernel32.dll", &["GetModuleFileNameA"]),
        64,
        options,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, module: u32, output: u32, size: u32) {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    for (i, value) in [0x0040_1000, module, output, size].iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(p: &mut Process32, module: u32, output: u32, size: u32) -> u32 {
    prepare(p, module, output, size);
    let mut before = p.cpu;
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    before.eip = 0x0040_1000;
    before.set_register(Register32::Eax, value);
    before.set_register(Register32::Esp, STACK + 16);
    assert_eq!(p.cpu, before);
    value
}

fn bytes(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut data = vec![0; length];
    p.memory.read(u64::from(address), &mut data).unwrap();
    data
}

#[test]
fn main_and_builtin_names_obey_xp_truncation_and_exact_write_footprints() {
    let mut p = process(ProcessOptions::default());
    let path = b"C:\\program.exe";
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    for module in [0, 0x0040_0000] {
        for size in [0, 1, 7, 14, 15, 100, u32::MAX] {
            p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
            p.memory.write(u64::from(OUTPUT), &[0xaa; 32]).unwrap();
            assert_eq!(call(&mut p, module, OUTPUT, size), size.min(14));
            let count = usize::try_from(size.min(15)).unwrap();
            let mut expected = [0xaa; 32];
            let mut full = path.to_vec();
            full.push(0);
            expected[..count].copy_from_slice(&full[..count]);
            assert_eq!(bytes(&p, OUTPUT, 32), expected);
            assert_eq!(p.last_error().unwrap(), if size < 15 { 0 } else { 99 });
        }
    }
    assert_eq!(call(&mut p, 0, u32::MAX, 0), 0);
    let path = b"C:\\Windows\\System32\\kernel32.dll\0";
    assert_eq!(call(&mut p, 0x7000_0800, OUTPUT, 64), 32);
    assert_eq!(bytes(&p, OUTPUT, path.len()), path);
    let snapshot = bytes(&p, OUTPUT, 64);
    assert_eq!(call(&mut p, 0x0040_0001, OUTPUT, 64), 0);
    assert_eq!(p.last_error().unwrap(), 126);
    assert_eq!(bytes(&p, OUTPUT, 64), snapshot);
}

#[test]
fn output_and_frame_faults_preserve_every_byte_and_cpu_state() {
    let mut p = process(ProcessOptions::default());
    for (output, size) in [
        (0, 1),
        (0x6000_0000, 20),
        (u32::MAX - 4, 20),
        (0x0040_2ff8, 20),
    ] {
        p.memory.write(u64::from(OUTPUT), &[0xaa; 32]).unwrap();
        p.memory.write(0x0040_2ff8, &[0xab; 8]).unwrap();
        prepare(&mut p, 0, output, size);
        let before = p.cpu;
        let pages = p.memory.mapped_pages();
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(bytes(&p, OUTPUT, 32), [0xaa; 32]);
        assert_eq!(bytes(&p, 0x0040_2ff8, 8), [0xab; 8]);
    }
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0, OUTPUT, 20);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    for stack in [0x1000_fff4, u32::MAX - 11] {
        prepare(&mut p, 0, OUTPUT, 20);
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
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 7, OUTPUT, 20);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;

#[test]
fn registered_image_paths_are_owned_and_independent_of_command_line() {
    use ring3_core::execution::GuestModule;
    let mut library = dll_executable::dll(0x5000_0000, &[], None);
    dll_executable::put(&mut library, 0xa8, 0);
    let mut path = b"D:\\Games\\Alpha App.exe".to_vec();
    let mut p = process(ProcessOptions {
        image_path: &path,
        command_line: b"unrelated.exe --option",
        modules: &[GuestModule {
            name: "Plugin.DLL",
            bytes: &library,
        }],
        ..ProcessOptions::default()
    });
    path.fill(b'x');
    assert_eq!(call(&mut p, 0, OUTPUT, 100), 22);
    assert_eq!(bytes(&p, OUTPUT, 23), b"D:\\Games\\Alpha App.exe\0");
    assert_eq!(call(&mut p, 0x5000_0000, OUTPUT, 100), 19);
    assert_eq!(bytes(&p, OUTPUT, 20), b"D:\\Games\\Plugin.DLL\0");
    let mut other = process(ProcessOptions::default());
    assert_eq!(call(&mut other, 0, OUTPUT, 100), 14);
    assert_eq!(bytes(&other, OUTPUT, 15), b"C:\\program.exe\0");
}

#[test]
fn invalid_and_oversized_image_metadata_is_rejected_before_mapping() {
    use ring3_core::execution::{GuestModule, LoadError};
    let exe = imported_executable::pe32(&[0xcc], "kernel32.dll", &["GetModuleFileNameA"]);
    for path in [
        b"".as_slice(),
        b"file.exe",
        b"C:file.exe",
        b"\\\\server\\file.exe",
        b"1:\\a.exe",
        b"C:\\",
        b"C:\\a\\\\b",
        b"C:\\.\\a",
        b"C:\\..\\a",
        b"C:\\a/b",
        b"C:\\a:b",
        b"C:\\a?b",
        b"C:\\a*b",
        b"C:\\a\"b",
        b"C:\\a<b",
        b"C:\\a>b",
        b"C:\\a|b",
        b"C:\\a\0b",
        b"C:\\\xff.exe",
    ] {
        assert!(matches!(
            Process32::load_with_options(
                &exe,
                0,
                ProcessOptions {
                    image_path: path,
                    ..ProcessOptions::default()
                }
            ),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
    let mut path = b"C:\\".to_vec();
    path.resize(32767, b'a');
    assert!(
        Process32::load_with_options(
            &exe,
            64,
            ProcessOptions {
                image_path: &path,
                ..ProcessOptions::default()
            }
        )
        .is_ok()
    );
    path.push(b'a');
    assert!(matches!(
        Process32::load_with_options(
            &exe,
            64,
            ProcessOptions {
                image_path: &path,
                ..ProcessOptions::default()
            }
        ),
        Err(LoadError::InvalidProcessParameters)
    ));
    path.truncate(32761);
    path.extend_from_slice(b"\\x.exe");
    let library = dll_executable::dll(0x5000_0000, &[], None);
    assert!(matches!(
        Process32::load_with_options(
            &exe,
            64,
            ProcessOptions {
                image_path: &path,
                modules: &[GuestModule {
                    name: "long.dll",
                    bytes: &library
                }],
                ..ProcessOptions::default()
            }
        ),
        Err(LoadError::InvalidProcessParameters)
    ));
}

#[test]
fn aliases_use_captured_arguments_and_checked_last_error_precedence() {
    let mut p = process(ProcessOptions::default());
    assert_eq!(call(&mut p, 0, STACK + 4, 15), 14);
    assert_eq!(bytes(&p, STACK + 4, 15), b"C:\\program.exe\0");
    prepare(&mut p, 0, STACK, 15);
    let result = p.run(1);
    assert_eq!(result.api_calls, 1);
    assert_eq!(p.cpu.eip, u32::from_le_bytes(*b"C:\\p"));
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 16);
    assert_eq!(call(&mut p, 0, 0x7ffd_e034, 15), 14);
    assert_eq!(p.last_error().unwrap(), u32::from_le_bytes(*b"C:\\p"));
    assert_eq!(call(&mut p, 0, 0x7ffd_e034, 4), 4);
    assert_eq!(p.last_error().unwrap(), 0);
    p.memory.write(u64::from(OUTPUT), &[0xab; 32]).unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    for size in [0, 4, 14] {
        prepare(&mut p, 0, OUTPUT, size);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, OUTPUT, 32), [0xab; 32]);
    }
    assert_eq!(call(&mut p, 0, OUTPUT, 15), 14);
}

#[test]
fn exact_output_span_supports_cross_page_unaligned_and_top_address_buffers() {
    let mut p = process(ProcessOptions::default());
    p.memory
        .map_zeroed(0x6000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for address in [OUTPUT + 1, 0x6000_0ff8, 0xffff_fff1] {
        assert_eq!(call(&mut p, 0, address, u32::MAX), 14);
        assert_eq!(bytes(&p, address, 15), b"C:\\program.exe\0");
    }
    p.memory
        .protect(
            0x6000_0000,
            8192,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    assert_eq!(call(&mut p, 0, 0x6000_0ff8, 15), 14);
}

#[path = "support/module_file_name_executable.rs"]
mod module_file_name_executable;

#[test]
fn module_file_name_guest_matches_whole_and_single_instruction_execution() {
    let image = module_file_name_executable::pe32();
    let mut whole = Process32::load(&image, 25).unwrap();
    let mut stepped = Process32::load(&image, 25).unwrap();
    let result = whole.run(40);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (11, 2));
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
    assert_eq!((instructions, apis), (11, 2));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 3);
    assert_eq!(whole.cpu.register(Register32::Ebx), 14);
    assert_eq!(
        whole.cpu.register(Register32::Ecx),
        u32::from_le_bytes(*b"C:\\p")
    );
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(bytes(&whole, OUTPUT, 15), b"C:\\program.exe\0");
    assert_eq!(bytes(&whole, 0x0040_21c0, 3), b"C:\\");
    assert_eq!(bytes(&whole, OUTPUT, 128), bytes(&stepped, OUTPUT, 128));
}
