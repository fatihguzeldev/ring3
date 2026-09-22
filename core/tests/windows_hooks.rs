#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/hook_executable.rs"]
mod hook_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const SET: u32 = 0x7000_024c;
const REMOVE: u32 = 0x7000_0250;
const STACK: u32 = 0x1000_ff00;
const ERROR: u32 = 0x7ffd_e034;
const FIRST: u32 = 0x7400_0004;
const KEYBOARD: [u32; 4] = [13, 0xdead_beef, 0x0040_0000, 0];
const ARGS: [u32; 4] = [u32::MAX, 0xdead_beef, 0, 1];
const CBT: [u32; 4] = [5, 0xdead_beef, 0, 1];

fn load() -> Process32 {
    Process32::load(&hook_executable::pe32(), 32).unwrap()
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
        put(p, STACK + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn imported_hook_lifetime_runs_whole_or_stepwise() {
    for bytes in [
        hook_executable::pe32(),
        hook_executable::keyboard(),
        hook_executable::cbt(),
    ] {
        for budget in [1, 100] {
            let mut p = Process32::load(&bytes, 32).unwrap();
            let mut counts = (0, 0);
            loop {
                let result = p.run(budget);
                counts.0 += result.instructions;
                counts.1 += result.api_calls;
                if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 100);
            }
            assert_eq!(counts, (11, 3));
            assert_eq!(p.cpu.register(Register32::Eax), 0);
            assert_eq!(p.cpu.register(Register32::Ebx), FIRST);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(p.last_error().unwrap(), 1404);
        }
    }
}

#[test]
fn duplicate_callbacks_have_independent_process_local_lifetimes() {
    let mut p = load();
    let mut other = load();
    put(&mut p, ERROR, 77);
    put(&mut p, 0x7000_2020, 88);
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, SET, &ARGS), FIRST);
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 4);
    assert_eq!(call(&mut other, REMOVE, &[FIRST]), 0);
    assert_eq!(other.last_error().unwrap(), 1404);
    assert_eq!(call(&mut other, SET, &ARGS), FIRST);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 0);
    assert_eq!(p.last_error().unwrap(), 1404);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 4]), 1);
    assert_eq!(call(&mut other, REMOVE, &[FIRST]), 1);
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 8);
    assert_eq!(call(&mut p, 0x7000_021c, &[FIRST + 8]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 8]), 1);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(errno, 88_u32.to_le_bytes());
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn capacity_recovery_does_not_recycle_stale_handles() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    for index in 0..4096 {
        let args = match index % 3 {
            0 => ARGS,
            1 => KEYBOARD,
            _ => CBT,
        };
        assert_eq!(call(&mut p, SET, &args), FIRST + index * 4);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, SET, &ARGS);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, SET, &ARGS), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    for index in 0..4096 {
        assert_eq!(call(&mut p, REMOVE, &[FIRST + index * 4]), 1);
    }
    for index in 4096..8192 {
        let args = match index % 3 {
            0 => ARGS,
            1 => KEYBOARD,
            _ => CBT,
        };
        assert_eq!(call(&mut p, SET, &args), FIRST + index * 4);
    }
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 0);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn unsupported_forms_and_error_faults_preserve_registration_identity() {
    let mut p = load();
    for args in [
        [0, 1, 0, 1],
        [3, 1, 0, 1],
        [u32::MAX, 1, 1, 1],
        [u32::MAX, 1, 0, 0],
        [u32::MAX, 1, 0, 2],
        [5, 1, 1, 1],
        [5, 1, 0, 0],
        [5, 1, 0, 2],
    ] {
        let before = prepare(&mut p, SET, &args);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, SET, &ARGS);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    for (api, args) in [
        (SET, vec![u32::MAX, 0, 0, 1]),
        (SET, vec![5, 0, 0, 1]),
        (REMOVE, vec![FIRST]),
    ] {
        let before = prepare(&mut p, api, &args);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, SET, &ARGS), FIRST);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, SET, &[u32::MAX, 0, 0, 1]), 0);
    assert_eq!(p.last_error().unwrap(), 1427);
    for invalid in [0, 1, FIRST, FIRST + 1, 0x7200_0004, 0x7300_0004, u32::MAX] {
        assert_eq!(call(&mut p, REMOVE, &[invalid]), 0);
        assert_eq!(p.last_error().unwrap(), 1404);
    }
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 4);
}

#[test]
fn incomplete_frames_do_not_install_or_remove_and_error_aliases_follow_guest_writes() {
    let mut p = load();
    assert_eq!(call(&mut p, SET, &ARGS), FIRST);
    for (api, args) in [(SET, ARGS.to_vec()), (REMOVE, vec![FIRST])] {
        prepare(&mut p, api, &args);
        p.cpu.set_register(
            Register32::Esp,
            0x1001_0000 - u32::try_from(args.len()).unwrap() * 4,
        );
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 4);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    prepare(&mut p, REMOVE, &[0]);
    p.cpu.set_register(Register32::Esp, ERROR);
    put(&mut p, ERROR, 0x0040_1000);
    put(&mut p, ERROR + 4, 0);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 1404);
    assert_eq!(p.cpu.register(Register32::Esp), ERROR + 8);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 4]), 1);
}

#[test]
fn keyboard_hooks_accept_owned_modules_and_keep_process_local_lifetimes() {
    use ring3_core::execution::{GuestModule, ProcessOptions};
    let library = dll_executable::dll(0x5000_0000, &[0xb8, 1, 0, 0, 0, 0xc2, 12, 0], None);
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &library,
    }];
    let mut p = Process32::load_with_options(
        &hook_executable::keyboard(),
        64,
        ProcessOptions {
            modules: &modules,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    let mut other = load();
    let pages = p.memory.mapped_pages();
    put(&mut p, ERROR, 77);
    put(&mut p, 0x7000_2020, 88);
    for (index, module) in [0, 0x0040_0000, 0x5000_0000, 0x7000_080c]
        .into_iter()
        .enumerate()
    {
        let handle = FIRST + u32::try_from(index).unwrap() * 4;
        assert_eq!(call(&mut p, SET, &[13, 0xdead_beef, module, 0]), handle);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 16);
    assert_eq!(call(&mut other, REMOVE, &[FIRST]), 0);
    assert_eq!(call(&mut other, SET, &KEYBOARD), FIRST);
    for index in [2, 0, 4, 3, 1] {
        assert_eq!(call(&mut p, REMOVE, &[FIRST + index * 4]), 1);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    assert_eq!(call(&mut other, REMOVE, &[FIRST]), 1);
    assert_eq!(call(&mut p, SET, &KEYBOARD), FIRST + 20);
    assert_eq!(p.memory.mapped_pages(), pages);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(errno, 88_u32.to_le_bytes());
}

#[test]
fn keyboard_scope_and_callback_errors_do_not_consume_handles() {
    let mut p = load();
    for (args, error) in [
        ([13, 0, 0, 0], 1427),
        ([13, 0, 0x0040_0000, 1], 1427),
        ([13, 1, 0, 1], 1429),
        ([13, 1, 0x0040_0000, u32::MAX], 1429),
    ] {
        assert_eq!(call(&mut p, SET, &args), 0);
        assert_eq!(p.last_error().unwrap(), error);
    }
    for args in [[13, 1, 1, 0], [13, 1, 0x0040_0001, 0], [14, 1, 0, 0]] {
        let before = prepare(&mut p, SET, &args);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 1429);
    }
    assert_eq!(call(&mut p, SET, &KEYBOARD), FIRST);
}

#[test]
fn keyboard_registration_does_not_read_callbacks_or_error_cells_on_success() {
    let mut p = load();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    for args in [[13, 0, 0, 0], [13, 1, 0, 1]] {
        let before = prepare(&mut p, SET, &args);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for (index, callback) in [1, 0x0040_1000, u32::MAX].into_iter().enumerate() {
        let handle = FIRST + u32::try_from(index).unwrap() * 4;
        assert_eq!(call(&mut p, SET, &[13, callback, 0, 0]), handle);
        assert_eq!(call(&mut p, REMOVE, &[handle]), 1);
    }
}

#[test]
fn keyboard_zero_budget_and_incomplete_frame_leave_registration_unchanged() {
    let mut p = load();
    let before = prepare(&mut p, SET, &KEYBOARD);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, SET, &KEYBOARD), FIRST);
    prepare(&mut p, REMOVE, &[FIRST]);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
}

#[test]
fn cbt_lifetimes_do_not_read_callbacks_and_preserve_other_hook_kinds() {
    let mut p = load();
    assert_eq!(call(&mut p, SET, &[5, 0, 0, 1]), 0);
    assert_eq!(p.last_error().unwrap(), 1427);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, SET, &CBT);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    for (index, callback) in [1, 0x0040_1000, u32::MAX].into_iter().enumerate() {
        assert_eq!(
            call(&mut p, SET, &[5, callback, 0, 1]),
            FIRST + u32::try_from(index).unwrap() * 4
        );
    }
    assert_eq!(call(&mut p, SET, &ARGS), FIRST + 12);
    assert_eq!(call(&mut p, SET, &KEYBOARD), FIRST + 16);
    for index in [1, 3, 0, 4, 2] {
        assert_eq!(call(&mut p, REMOVE, &[FIRST + index * 4]), 1);
    }
    assert_eq!(call(&mut p, SET, &CBT), FIRST + 20);
}
