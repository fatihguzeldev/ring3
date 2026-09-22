#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/procedure_executable.rs"]
mod procedure_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0274;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_21a0;
const ERROR: u32 = 0x7ffd_e034;

fn load() -> Process32 {
    Process32::load(&procedure_executable::pe32(), 32).unwrap()
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, module: u32, name: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, word) in [0x0040_1000, module, name].into_iter().enumerate() {
        put(p, STACK + u32::try_from(index).unwrap() * 4, word);
    }
    p.cpu
}

fn call(p: &mut Process32, module: u32, name: u32) -> u32 {
    let mut expected = prepare(p, module, name);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, result);
    expected.set_register(Register32::Esp, STACK + 12);
    assert_eq!(p.cpu, expected);
    result
}

#[test]
fn imported_dynamic_call_runs_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = load();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 20);
        }
        assert_eq!(counts, (8, 3));
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7000_0030);
        assert_eq!(p.cpu.register(Register32::Eax), 0x0a28_0105);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn name(p: &mut Process32, value: &[u8]) {
    p.memory.write(u64::from(NAME), value).unwrap();
    p.memory
        .write(u64::from(NAME) + value.len() as u64, &[0])
        .unwrap();
}

#[test]
fn builtin_function_and_data_addresses_share_static_identity() {
    let mut p = load();
    let mut other = load();
    let pages = p.memory.mapped_pages();
    put(&mut p, ERROR, 77);
    put(&mut p, 0x7000_2020, 88);
    for (module, symbol, address) in [
        (0x800, "GetVersion", 0x7000_0030),
        (0x800, "GetProcAddress", API),
        (0x804, "_errno", 0x7000_0124),
        (0x804, "__argc", 0x7000_2010),
        (0x804, "__argv", 0x7000_2014),
        (0x808, "Direct3DCreate8", 0x7000_000c),
        (0x80c, "GetDesktopWindow", 0x7000_0010),
        (0x810, "GetDeviceCaps", 0x7000_00a8),
        (0x814, "timeGetTime", 0x7000_0244),
    ] {
        name(&mut p, symbol.as_bytes());
        assert_eq!(call(&mut p, 0x7000_0000 + module, NAME), address);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    name(&mut p, b"__argc");
    let argc = call(&mut p, 0x7000_0804, NAME);
    put(&mut p, argc, 19);
    assert_eq!(call(&mut p, 0x7000_0804, NAME), argc);
    name(&mut other, b"__argc");
    assert_eq!(call(&mut other, 0x7000_0804, NAME), argc);
    let mut bytes = [0; 4];
    p.memory.read(u64::from(argc), &mut bytes).unwrap();
    assert_eq!(bytes, 19_u32.to_le_bytes());
    other.memory.read(u64::from(argc), &mut bytes).unwrap();
    assert_eq!(bytes, 1_u32.to_le_bytes());
    p.memory.read(0x7000_2020, &mut bytes).unwrap();
    assert_eq!(bytes, 88_u32.to_le_bytes());
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn unimplemented_names_ordinals_and_guest_images_remain_explicit() {
    let mut p = load();
    put(&mut p, ERROR, 77);
    for symbol in [
        b"getversion".as_slice(),
        b"MissingFunction",
        b"RaiseException",
        b"",
        b"\xff",
    ] {
        name(&mut p, symbol);
        let before = prepare(&mut p, 0x7000_0800, NAME);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    for (module, pointer) in [
        (0, NAME),
        (0x0040_0000, NAME),
        (0x7000_0800, 0),
        (0x7000_0800, 65535),
    ] {
        let before = prepare(&mut p, module, pointer);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn missing_handles_and_error_page_faults_leave_lookup_state_unchanged() {
    let mut p = load();
    for handle in [1, 0x7000_0801, 0x7400_0004, u32::MAX] {
        assert_eq!(call(&mut p, handle, u32::MAX), 0);
        assert_eq!(p.last_error().unwrap(), 126);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, 1, NAME);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, 0x7000_0800, NAME), 0x7000_0030);
    name(&mut p, b"__argc");
    assert_eq!(call(&mut p, 0x7000_0804, NAME), 0x7000_2010);
}

#[test]
fn name_boundaries_and_faults_do_not_change_cpu_or_errors() {
    let mut p = load();
    put(&mut p, ERROR, 77);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fff5, b"GetVersion\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0800, 0xffff_fff5), 0x7000_0030);
    p.memory.write(0xffff_fffc, b"abcd").unwrap();
    for pointer in [0xffff_fffc, 0xdead_beef] {
        let before = prepare(&mut p, 0x7000_0800, pointer);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    p.memory.write(0xffff_f000, &vec![b'a'; 4096]).unwrap();
    let before = prepare(&mut p, 0x7000_0800, 0xffff_f000);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, 0x7000_0800, NAME);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn zero_budget_and_incomplete_frames_do_not_dispatch_lookup() {
    let mut p = load();
    let before = prepare(&mut p, 0x7000_0800, NAME);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, 0x7000_0800, NAME), 0x7000_0030);
}

#[test]
fn supplied_module_identity_does_not_use_builtin_exports() {
    use ring3_core::execution::{GuestModule, ProcessOptions};
    let library = dll_executable::dll(0x5000_0000, &[0xb8, 1, 0, 0, 0, 0xc2, 12, 0], None);
    let modules = [GuestModule {
        name: "kernel32.dll",
        bytes: &library,
    }];
    let program = imported_executable::pe32(&[0xcc], "msvcrt.dll", &["_errno"]);
    let mut p = Process32::load_with_options(
        &program,
        64,
        ProcessOptions {
            modules: &modules,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    name(&mut p, b"GetVersion");
    let before = prepare(&mut p, 0x5000_0000, NAME);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, 0x7000_0800, NAME), 0);
    assert_eq!(p.last_error().unwrap(), 126);
}
