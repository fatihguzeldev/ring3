#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/modules_executable.rs"]
mod modules_executable;

use ring3_core::execution::{
    GuestModule, LoadError, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const NAME: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;
const DLL: u32 = 0x5000_0000;

#[test]
fn guest_calls_resume_with_the_same_module_state_and_budget() {
    let mut whole = Process32::load(&modules_executable::pe32(), 32).unwrap();
    let mut stepped = Process32::load(&modules_executable::pe32(), 32).unwrap();
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
}

fn program() -> Vec<u8> {
    imported_executable::pe32(
        &[0xcc],
        "kernel32.dll",
        &["LoadLibraryA", "GetModuleHandleA", "FreeLibrary"],
    )
}

fn load(modules: &[GuestModule<'_>]) -> Process32 {
    Process32::load_with_options(
        &program(),
        64,
        ProcessOptions {
            modules,
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
fn builtin_identity_name_rules_and_balanced_references() {
    let mut process = load(&[]);
    process
        .memory
        .write(0x7ffd_e034, &99_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, 1, 0), 0x0040_0000);
    for module in ["kernel32.dll", "msvcrt.dll", "d3d8.dll", "user32.dll"] {
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
    for value in ["unknown", "kernel32.", &"a".repeat(255)] {
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
        "c:\\msvcrt.dll",
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
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let modules = [GuestModule {
        name: "msvcrt.dll",
        bytes: &library,
    }];
    let mut process = load(&modules);
    let bootstrap = process.cpu;
    name(&mut process, "MSVCRT.DLL");
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
