#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_notifications_executable.rs"]
mod thread_notifications_executable;

use ring3_core::execution::{
    GuestModule, LoadError, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop,
    Register32, StopReason,
};

const NAME: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;
const DLL: u32 = 0x5000_0000;

fn program() -> Vec<u8> {
    imported_executable::pe32(
        &[0xcc],
        "kernel32.dll",
        &["LoadLibraryA", "GetModuleHandleA", "FreeLibrary"],
    )
}

fn load(library: &[u8]) -> Process32 {
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: library,
    }];
    Process32::load_with_options(
        &program(),
        64,
        ProcessOptions {
            deferred_modules: &modules,
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

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(process: &mut Process32, slot: u32) {
    let mut address = [0; 4];
    process
        .memory
        .read(0x0040_2060 + u64::from(slot) * 4, &mut address)
        .unwrap();
    process.cpu.eip = u32::from_le_bytes(address);
    process.cpu.set_register(Register32::Esp, STACK);
    for (offset, value) in [(0, 0x0040_1000_u32), (4, NAME)] {
        process
            .memory
            .write(u64::from(STACK + offset), &value.to_le_bytes())
            .unwrap();
    }
}

fn call(process: &mut Process32, slot: u32) -> u32 {
    prepare(process, slot);
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    process.cpu.register(Register32::Eax)
}

#[test]
fn deferred_module_is_mapped_without_running_startup_attach() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &library,
    }];
    let process = Process32::load_with_options(
        &program(),
        64,
        ProcessOptions {
            deferred_modules: &modules,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(word(&process, DLL + 0x2180), 0);
}

#[test]
fn static_and_deferred_inputs_share_name_and_dependency_validation() {
    let first = dll_executable::dll(DLL, &dll_executable::attach(DLL, 1, true), None);
    let second = dll_executable::dll(
        DLL + 0x0001_0000,
        &dll_executable::attach(DLL + 0x0001_0000, 2, true),
        Some("first.dll"),
    );
    assert!(matches!(
        Process32::load_with_options(
            &program(),
            96,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "first.dll",
                    bytes: &first,
                }],
                deferred_modules: &[GuestModule {
                    name: "FIRST.dll",
                    bytes: &first,
                }],
                ..ProcessOptions::default()
            },
        ),
        Err(LoadError::DuplicateModule { .. })
    ));
    assert!(matches!(
        Process32::load_with_options(
            &program(),
            96,
            ProcessOptions {
                deferred_modules: &[
                    GuestModule {
                        name: "second.dll",
                        bytes: &second,
                    },
                    GuestModule {
                        name: "first.dll",
                        bytes: &first,
                    },
                ],
                ..ProcessOptions::default()
            },
        ),
        Err(LoadError::DeferredModuleDependency { .. })
    ));
}

#[test]
fn first_load_runs_dynamic_attach_and_resumes_across_budgets() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let mut whole = load(&library);
    let mut stepped = load(&library);
    for process in [&mut whole, &mut stepped] {
        name(process, "demo.dll");
        assert_eq!(call(process, 1), 0);
        assert_eq!(process.last_error().unwrap(), 126);
        prepare(process, 0);
    }
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    let mut counts = (0, 0);
    loop {
        let step = stepped.run(1);
        counts.0 += step.instructions;
        counts.1 += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(counts, (result.instructions, result.api_calls));
    assert_eq!(stepped.cpu, whole.cpu);
    for process in [&mut whole, &mut stepped] {
        assert_eq!(process.cpu.register(Register32::Eax), DLL);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 8);
        assert_eq!(word(process, DLL + 0x2180), 42);
        assert_eq!(word(process, DLL + 0x2190), DLL);
        assert_eq!(word(process, DLL + 0x2194), 1);
        assert_eq!(word(process, DLL + 0x2198), 0);
        assert_eq!(call(process, 1), DLL);
        assert_eq!(call(process, 0), DLL);
        assert_eq!(word(process, DLL + 0x2180), 42);
    }
}

#[test]
fn failed_dynamic_attach_stays_unpublished_and_reports_loader_error() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 99, false), None);
    let mut process = load(&library);
    name(&mut process, "demo.dll");
    assert_eq!(call(&mut process, 0), 0);
    assert_eq!(process.last_error().unwrap(), 1114);
    assert_eq!(word(&process, DLL + 0x2180), 99);
    assert_eq!(call(&mut process, 1), 0);
    assert_eq!(process.last_error().unwrap(), 126);
    assert_eq!(call(&mut process, 0), 0);
    assert_eq!(process.last_error().unwrap(), 1114);
}

#[test]
fn entryless_deferred_module_publishes_without_a_guest_callback() {
    let mut library = dll_executable::dll(DLL, &[], None);
    dll_executable::put(&mut library, 0xa8, 0);
    let mut process = load(&library);
    name(&mut process, "demo.dll");
    prepare(&mut process, 0);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Eax), DLL);
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(word(&process, DLL + 0x2180), 0);
    assert_eq!(call(&mut process, 1), DLL);
}

#[test]
fn dynamic_attach_can_call_module_apis_before_publication_completes() {
    let library = thread_notifications_executable::dll();
    let mut process = load(&library);
    name(&mut process, "demo.dll");
    assert_eq!(call(&mut process, 0), DLL);
    assert_eq!(call(&mut process, 1), DLL);
}

#[test]
fn callback_setup_fault_leaves_deferred_module_retryable() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let mut process = load(&library);
    name(&mut process, "demo.dll");
    prepare(&mut process, 0);
    process
        .memory
        .protect(u64::from(STACK & !0xfff), PAGE_SIZE, Permissions::READ)
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
        .protect(
            u64::from(STACK & !0xfff),
            PAGE_SIZE,
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert_eq!(call(&mut process, 1), 0);
    assert_eq!(call(&mut process, 0), DLL);
}

#[test]
fn failed_attach_error_write_fault_retains_callback_for_retry() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 99, false), None);
    let mut process = load(&library);
    name(&mut process, "demo.dll");
    prepare(&mut process, 0);
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let result = process.run(100);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu.eip, 0x7000_0ff8);
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
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 1114);
    assert_eq!(call(&mut process, 1), 0);
    assert_eq!(call(&mut process, 0), 0);
    assert_eq!(process.last_error().unwrap(), 1114);
}
