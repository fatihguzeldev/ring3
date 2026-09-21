#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/modules_executable.rs"]
mod modules_executable;
#[path = "support/thread_notifications_executable.rs"]
mod thread_notifications_executable;

use ring3_core::execution::{
    GuestModule, LoadError, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const NAME: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;
const DLL: u32 = 0x5000_0000;

#[test]
fn guest_calls_resume_with_the_same_module_state_and_budget() {
    for bytes in [modules_executable::pe32(), modules_executable::absolute()] {
        check_guest(&bytes);
    }
}

fn check_guest(bytes: &[u8]) {
    let mut whole = Process32::load(bytes, 32).unwrap();
    let mut stepped = Process32::load(bytes, 32).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 4);
    assert_eq!(
        whole.cpu.register(Register32::Ebx),
        whole.cpu.register(Register32::Esi)
    );
    assert_ne!(whole.cpu.register(Register32::Ebx), 0);
    assert_eq!(whole.cpu.register(Register32::Edi), 1);
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.last_error().unwrap(), 126);
    let mut instructions = 0;
    let mut calls = 0;
    for _ in 0..100 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions, result.api_calls)
    );
    assert_eq!(stepped.cpu, whole.cpu);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}

fn program() -> Vec<u8> {
    imported_executable::pe32(
        &[0xcc],
        "kernel32.dll",
        &[
            "LoadLibraryA",
            "GetModuleHandleA",
            "FreeLibrary",
            "DisableThreadLibraryCalls",
        ],
    )
}

fn load(modules: &[GuestModule<'_>]) -> Process32 {
    load_path(modules, b"C:\\program.exe")
}

fn load_path(modules: &[GuestModule<'_>], image_path: &[u8]) -> Process32 {
    Process32::load_with_options(
        &program(),
        64,
        ProcessOptions {
            modules,
            image_path,
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn name(process: &mut Process32, value: &str) {
    process
        .memory
        .write(u64::from(NAME), value.as_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(NAME) + value.len() as u64, &[0])
        .unwrap();
}

fn prepare(process: &mut Process32, slot: u32, argument: u32) {
    let mut address = [0; 4];
    process
        .memory
        .read(0x0040_2060 + u64::from(slot) * 4, &mut address)
        .unwrap();
    process.cpu.eip = u32::from_le_bytes(address);
    process.cpu.set_register(Register32::Esp, STACK);
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK + 4), &argument.to_le_bytes())
        .unwrap();
}

fn call(process: &mut Process32, slot: u32, argument: u32) -> u32 {
    prepare(process, slot, argument);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 8);
    process.cpu.register(Register32::Eax)
}

#[test]
fn dll_process_attach_can_disable_thread_notifications_and_still_finish() {
    let library = thread_notifications_executable::dll();
    let bytes = dll_executable::exe("demo.dll");
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &library,
    }];
    let options = ProcessOptions {
        modules: &modules,
        ..ProcessOptions::default()
    };
    let mut whole = Process32::load_with_options(&bytes, 32, options).unwrap();
    let mut stepped = Process32::load_with_options(&bytes, 32, options).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(whole.cpu.register(Register32::Eax), 1);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..100 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions, result.api_calls)
    );
    assert_eq!(stepped.cpu, whole.cpu);
}

#[test]
fn notification_disable_checks_module_identity_and_preserves_error_state_on_success() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let mut process = load(&[GuestModule {
        name: "demo.dll",
        bytes: &library,
    }]);
    let bootstrap = process.cpu;
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    for handle in [DLL, DLL, 0x7000_0800] {
        assert_eq!(call(&mut process, 3, handle), 1);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    name(&mut process, "demo.dll");
    prepare(&mut process, 0, NAME);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
    process.cpu = bootstrap;
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(call(&mut process, 0, NAME), DLL);
    assert_eq!(call(&mut process, 2, DLL), 1);
    prepare(&mut process, 2, DLL);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
    for invalid in [0, DLL + 1, 0x6000_0000, u32::MAX] {
        assert_eq!(call(&mut process, 3, invalid), 0);
        assert_eq!(process.last_error().unwrap(), 126);
    }
    prepare(&mut process, 3, 0x0040_0000);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi {
            address: 0x7000_00fc
        }
    );
    assert_eq!(process.cpu, before);
    process
        .memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, 3, DLL), 1);
    prepare(&mut process, 3, 0);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    prepare(&mut process, 3, DLL);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}

#[test]
fn builtin_identity_name_rules_and_balanced_references() {
    let mut process = load(&[]);
    process
        .memory
        .write(0x7ffd_e034, &99_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, 1, 0), 0x0040_0000);
    for module in [
        "kernel32.dll",
        "msvcrt.dll",
        "d3d8.dll",
        "user32.dll",
        "gdi32.dll",
        "winmm.dll",
    ] {
        name(&mut process, module);
        let handle = call(&mut process, 1, NAME);
        assert_ne!(handle, 0);
        assert!(process.memory.read(u64::from(handle), &mut [0]).is_err());
        name(&mut process, &module.to_uppercase());
        assert_eq!(call(&mut process, 0, NAME), handle);
        name(&mut process, module.trim_end_matches(".dll"));
        assert_eq!(call(&mut process, 0, NAME), handle);
        name(&mut process, &format!("{module}."));
        assert_eq!(call(&mut process, 1, NAME), handle);
        assert_eq!(call(&mut process, 2, handle), 1);
        assert_eq!(call(&mut process, 2, handle), 1);
        prepare(&mut process, 2, handle);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        assert_eq!(process.cpu, before);
    }
    assert_eq!(process.last_error().unwrap(), 99);
}

#[test]
fn missing_modules_invalid_handles_and_unsupported_names_are_distinct() {
    let mut process = load(&[]);
    for value in ["unknown", "kernel32.", "c:\\msvcrt.dll", &"a".repeat(255)] {
        name(&mut process, value);
        for slot in [0, 1] {
            assert_eq!(call(&mut process, slot, NAME), 0);
            assert_eq!(process.last_error().unwrap(), 126);
        }
    }
    for handle in [0, 0x0040_0000, 0xdead_beef] {
        assert_eq!(call(&mut process, 2, handle), 0);
        assert_eq!(process.last_error().unwrap(), 6);
    }
    for value in [
        "",
        ".",
        "..",
        "c:msvcrt.dll",
        "./msvcrt.dll",
        "é.dll",
        &"a".repeat(256),
    ] {
        name(&mut process, value);
        prepare(&mut process, 0, NAME);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 6);
    }
}

#[test]
fn faults_do_not_acquire_references_or_change_cpu() {
    let mut process = load(&[]);
    name(&mut process, "msvcrt.dll");
    let handle = call(&mut process, 1, NAME);
    for pointer in [0, u32::MAX, 0x0040_2fff] {
        process.memory.write(0x0040_2fff, b"a").unwrap();
        prepare(&mut process, 0, pointer);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
    }
    prepare(&mut process, 0, NAME);
    process.cpu.set_register(Register32::Esp, u32::MAX - 3);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    name(&mut process, "missing.dll");
    prepare(&mut process, 0, NAME);
    process
        .memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    prepare(&mut process, 2, handle);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
}

#[test]
fn explicit_provider_precedence_and_bootstrap_readiness_are_preserved() {
    for module_name in ["msvcrt.dll", "winmm.dll"] {
        let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
        let modules = [GuestModule {
            name: module_name,
            bytes: &library,
        }];
        let mut process = load(&modules);
        let bootstrap = process.cpu;
        name(&mut process, &module_name.to_uppercase());
        assert_eq!(call(&mut process, 1, NAME), DLL);
        prepare(&mut process, 0, NAME);
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        // changing eip to the exe must not invent completed initialization.
        process.cpu.eip = 0x0040_1000;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        prepare(&mut process, 0, NAME);
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        process.cpu = bootstrap;
        for _ in 0..100 {
            if process.run(1).reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
        }
        assert_eq!(call(&mut process, 0, NAME), DLL);
        assert_eq!(call(&mut process, 2, DLL), 1);
        prepare(&mut process, 2, DLL);
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
    }
}

#[test]
fn zero_entry_provider_is_ready_without_bootstrap() {
    let mut library = dll_executable::dll(DLL, &[], None);
    dll_executable::put(&mut library, 0xa8, 0);
    let mut process = load(&[GuestModule {
        name: "demo.dll",
        bytes: &library,
    }]);
    name(&mut process, "demo.dll");
    assert_eq!(call(&mut process, 0, NAME), DLL);
}

#[test]
fn null_image_bases_cannot_be_used_as_successful_module_handles() {
    let mut program = program();
    dll_executable::put(&mut program, 0xb4, 0);
    assert!(matches!(
        Process32::load(&program, 64),
        Err(LoadError::InvalidLayout)
    ));
    let library = dll_executable::dll(0, &[], None);
    dll_executable::put(&mut program, 0xb4, 0x0040_0000);
    assert!(matches!(
        Process32::load_with_options(
            &program,
            64,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "demo.dll",
                    bytes: &library
                }],
                ..ProcessOptions::default()
            }
        ),
        Err(LoadError::InvalidLayout)
    ));
}

#[test]
fn absolute_paths_match_only_the_owned_identity_and_balance_references() {
    let mut p = load(&[]);
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for path in [
        r"C:\Windows\System32\kernel32.dll",
        r"c:\WINDOWS\system32\KERNEL32",
        r"C:\Windows\System32\kernel32.dll.",
    ] {
        name(&mut p, path);
        assert_eq!(call(&mut p, 1, NAME), 0x7000_0800);
        assert_eq!(call(&mut p, 0, NAME), 0x7000_0800);
        assert_eq!(call(&mut p, 2, 0x7000_0800), 1);
    }
    assert_eq!(p.last_error().unwrap(), 77);
    prepare(&mut p, 2, 0x7000_0800);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
    for path in [
        r"D:\Windows\System32\kernel32.dll",
        r"C:\Elsewhere\kernel32.dll",
        r"C:\Windows\System32\absent.dll",
    ] {
        name(&mut p, path);
        for api in [0, 1] {
            assert_eq!(call(&mut p, api, NAME), 0);
            assert_eq!(p.last_error().unwrap(), 126);
        }
    }
    name(&mut p, r"c:\PROGRAM.exe");
    assert_eq!(call(&mut p, 1, NAME), 0x0040_0000);
    prepare(&mut p, 0, NAME);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn provided_paths_keep_precedence_and_initialization_rules() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let modules = [GuestModule {
        name: "gdi32.dll",
        bytes: &library,
    }];
    let mut p = load_path(&modules, br"D:\Build.v1\Game Files\demo.exe");
    let bootstrap = p.cpu;
    name(&mut p, r"d:\build.v1\GAME Files\GDI32");
    assert_eq!(call(&mut p, 1, NAME), DLL);
    prepare(&mut p, 0, NAME);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
    assert_eq!(p.cpu, before);
    name(&mut p, r"C:\Windows\System32\gdi32.dll");
    assert_eq!(call(&mut p, 1, NAME), 0);
    assert_eq!(p.last_error().unwrap(), 126);
    p.cpu = bootstrap;
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    name(&mut p, r"D:\Build.v1\Game Files\gdi32.dll.");
    assert_eq!(call(&mut p, 0, NAME), DLL);
    assert_eq!(call(&mut p, 2, DLL), 1);
    name(&mut p, "gdi32.dll");
    assert_eq!(call(&mut p, 1, NAME), DLL);
    prepare(&mut p, 2, DLL);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
}

#[test]
fn conflicting_main_and_dll_paths_stop_before_reference_changes() {
    let mut library = dll_executable::dll(DLL, &[], None);
    dll_executable::put(&mut library, 0xa8, 0);
    let mut p = load(&[GuestModule {
        name: "program.exe",
        bytes: &library,
    }]);
    name(&mut p, r"C:\program.exe");
    for api in [0, 1] {
        prepare(&mut p, api, NAME);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(result.reason, ProcessStop::UnsupportedApi { .. }));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, 1, 0), 0x0040_0000);
    prepare(&mut p, 2, DLL);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
}

#[test]
fn absolute_path_grammar_scan_and_normalized_length_are_bounded() {
    let mut p = load(&[]);
    for path in [
        r"C:relative.dll",
        r"C:/demo.dll",
        r"\server\demo.dll",
        r"C:\dir\\demo.dll",
        r"C:\.\demo.dll",
        r"C:\..\demo.dll",
        r"C:\dir\",
        r"C:\dir\.",
        r"C:\dir\..",
        r"C:\dir\bad?.dll",
        "C:\\é\\demo.dll",
    ] {
        name(&mut p, path);
        prepare(&mut p, 0, NAME);
        let before = p.cpu;
        let result = p.run(1);
        assert!(
            matches!(result.reason, ProcessStop::UnsupportedApi { .. }),
            "{path}: {result:?}"
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .map_zeroed(0x2000_0000, 32768, Permissions::READ_WRITE)
        .unwrap();
    let path = format!("C:\\{}.dll\0", "a".repeat(32760));
    assert_eq!(path.len(), 32768);
    p.memory.write(0x2000_0000, path.as_bytes()).unwrap();
    assert_eq!(call(&mut p, 1, 0x2000_0000), 0);
    assert_eq!(p.last_error().unwrap(), 126);
    for length in [32767, 32768] {
        let mut path = format!("C:\\{}", "a".repeat(length - 3)).into_bytes();
        if length == 32767 {
            path.push(0);
        }
        p.memory.write(0x2000_0000, &path).unwrap();
        prepare(&mut p, 0, 0x2000_0000);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn absolute_path_reads_and_missing_error_writes_are_atomic() {
    let mut p = load(&[]);
    p.memory.write(0x0040_2ffc, br"C:\a").unwrap();
    prepare(&mut p, 0, 0x0040_2ffc);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    name(&mut p, r"C:\Windows\System32\unknown.dll");
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0, NAME);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    name(&mut p, r"C:\Windows\System32\kernel32.dll");
    assert_eq!(call(&mut p, 0, NAME), 0x7000_0800);
    assert_eq!(call(&mut p, 2, 0x7000_0800), 1);
    prepare(&mut p, 2, 0x7000_0800);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { .. }
    ));
}
