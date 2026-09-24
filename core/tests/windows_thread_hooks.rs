#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_hook_cases.rs"]
mod thread_hook_cases;

#[test]
fn scheduled_children_register_hooks_that_the_parent_can_remove() {
    thread_hook_cases::scheduled_hook_lifetimes();
}

#[path = "support/hook_chain_cases.rs"]
#[allow(dead_code)]
mod hook_chain_cases;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const SET: u32 = 0x7000_024c;
const REMOVE: u32 = 0x7000_0250;
const STACK: u32 = 0x1000_b000;
const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;
const FIRST: u32 = 0x7400_0004;
const CODE: u32 = 0x0040_1000;

fn put(p: &mut Process32, address: u32, words: &[u32]) {
    p.memory
        .write(
            u64::from(address),
            &words
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    put(p, STACK, &[CODE]);
    put(p, STACK + 4, args);
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let saved = p.cpu;
    let mut expected = prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = CODE;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK
            + if api == 0x7000_0548 {
                4
            } else {
                4 * u32::try_from(args.len() + 1).unwrap()
            },
    );
    assert_eq!(p.cpu, expected);
    p.cpu = saved;
    value
}

fn child(p: &mut Process32) -> u32 {
    let handle = call(p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_ne!(handle, 0);
    handle
}

fn stopped(p: &mut Process32, before: Cpu32) -> ProcessStop {
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    run.reason
}

#[test]
fn immutable_identity_controls_scope_and_closed_thread_handles_retain_hooks() {
    let mut p = Process32::load(&thread_hook_cases::executable(), 64).unwrap();
    let thread = child(&mut p);
    p.cpu.set_fs_base(CHILD);
    put(&mut p, CHILD + 0x24, &[1]);
    put(&mut p, CHILD + 0x34, &[77]);
    for args in [
        [u32::MAX, 1, 0, 1],
        [5, 0, 0, 1],
        [5, 1, 1, 2],
        [5, 1, 0, 0],
        [13, 0, 0, 0],
        [13, 1, 0, 2],
    ] {
        let before = prepare(&mut p, SET, &args);
        assert_eq!(
            stopped(&mut p, before),
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.last_error().unwrap(), 77);
    }
    assert_eq!(call(&mut p, SET, &[u32::MAX, 1, 0, 2]), FIRST);
    assert_eq!(call(&mut p, SET, &[5, 1, 0, 2]), FIRST + 4);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, 0x7000_021c, &[thread]), 1);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    p.cpu.set_fs_base(0x5000_0000);
    for (api, args) in [(REMOVE, vec![FIRST + 4]), (SET, vec![5, 1, 0, 1])] {
        let before = prepare(&mut p, api, &args);
        assert_eq!(
            stopped(&mut p, before),
            ProcessStop::UnsupportedApi { address: api }
        );
    }
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, SET, &[5, 1, 0, 2]), FIRST + 8);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 4]), 1);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 8]), 1);
}

#[test]
fn child_error_faults_and_incomplete_frames_are_retryable_without_handle_consumption() {
    let mut p = Process32::load(&thread_hook_cases::executable(), 64).unwrap();
    child(&mut p);
    put(&mut p, PRIMARY + 0x34, &[88]);
    p.cpu.set_fs_base(CHILD);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ)
        .unwrap();
    for (api, args) in [(REMOVE, vec![FIRST]), (SET, vec![u32::MAX, 0, 0, 2])] {
        let before = prepare(&mut p, api, &args);
        for _ in 0..2 {
            assert!(matches!(
                stopped(&mut p, before),
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
    }
    for (api, args) in [(SET, vec![5, 1, 0, 2]), (REMOVE, vec![FIRST])] {
        let before = prepare(&mut p, api, &args);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = p.cpu;
        assert!(matches!(
            stopped(&mut p, before),
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!(call(&mut p, SET, &[5, 1, 0, 2]), FIRST);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 0);
    assert_eq!(p.last_error().unwrap(), 1404);
    assert_eq!(call(&mut p, SET, &[5, 0, 0, 2]), 0);
    assert_eq!(p.last_error().unwrap(), 1427);
    assert_eq!(call(&mut p, SET, &[5, 1, 0, 2]), FIRST + 4);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(p.last_error().unwrap(), 88);
}

#[test]
fn foreign_hooks_are_skipped_at_window_entry_and_between_forwarded_callbacks() {
    for budget in [1, 1000] {
        let mut p = hook_chain_cases::process(0);
        let thread = child(&mut p);
        call(&mut p, REMOVE, &[FIRST + 8]);
        p.cpu.set_fs_base(CHILD);
        call(&mut p, SET, &[5, 0xdead_beef, 0, 2]);
        p.cpu.set_fs_base(PRIMARY);
        call(&mut p, SET, &[5, hook_chain_cases::NEW, 0, 1]);
        p.cpu.set_fs_base(CHILD);
        call(&mut p, SET, &[5, 0xdead_beef, 0, 2]);
        p.cpu.set_fs_base(PRIMARY);
        call(&mut p, 0x7000_021c, &[thread]);
        call(&mut p, SET, &[2, 0xdead_beef, 0x0040_0000, 1]);
        hook_chain_cases::run(&mut p, budget);
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
        assert_eq!(p.last_error().unwrap(), 77);
        let mut log = [0; 8];
        p.memory.read(hook_chain_cases::DATA, &mut log).unwrap();
        assert_eq!(
            log,
            [3_u32, 0x7500_0004]
                .map(u32::to_le_bytes)
                .concat()
                .as_slice()
        );
    }
}

#[test]
fn dialog_entry_ignores_a_newer_foreign_hook() {
    let mut p = Process32::load(&thread_hook_cases::executable(), 64).unwrap();
    child(&mut p);
    let primary_hook = CODE + 0x80;
    call(&mut p, SET, &[5, primary_hook, 0, 1]);
    p.cpu.set_fs_base(CHILD);
    call(&mut p, SET, &[5, 0xdead_beef, 0, 2]);
    p.cpu.set_fs_base(PRIMARY);
    call(&mut p, SET, &[2, 0xdead_beef, 0x0040_0000, 1]);
    let template = [
        1_u16,
        0xffff,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        100,
        50,
        0,
        0,
        u16::from(b'T'),
        0,
    ];
    p.memory
        .write(
            u64::from(thread_hook_cases::DATA),
            &template.map(u16::to_le_bytes).concat(),
        )
        .unwrap();
    prepare(
        &mut p,
        0x7000_0434,
        &[0x0040_0000, thread_hook_cases::DATA, 0, CODE + 0x90, 0],
    );
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, primary_hook);
    assert_eq!(p.window_snapshots().len(), 1);
}

#[test]
fn scheduled_children_register_thread_keyboard_hooks() {
    thread_hook_cases::scheduled_keyboard_hook_lifetimes();
}

#[test]
fn thread_keyboard_scope_uses_immutable_owner_and_caller_error_storage() {
    let mut p = Process32::load(&thread_hook_cases::executable(), 64).unwrap();
    let thread = child(&mut p);
    put(&mut p, PRIMARY + 0x34, &[88]);
    p.cpu.set_fs_base(CHILD);
    put(&mut p, CHILD + 0x24, &[1]);
    put(&mut p, CHILD + 0x34, &[77]);
    for args in [[2, 1, 0, 1], [2, 0, 0, 0], [2, 1, 1, 2]] {
        let before = prepare(&mut p, SET, &args);
        assert_eq!(
            stopped(&mut p, before),
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.last_error().unwrap(), 77);
    }
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, SET, &[2, 0, 0x0040_0000, 2]);
    assert!(matches!(
        stopped(&mut p, before),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(call(&mut p, SET, &[2, 1, 0x0040_0000, 2]), FIRST);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, SET, &[2, 0, 0, 2]), 0);
    assert_eq!(p.last_error().unwrap(), 1427);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(p.last_error().unwrap(), 88);
    assert_eq!(call(&mut p, 0x7000_021c, &[thread]), 1);
    assert_eq!(call(&mut p, REMOVE, &[FIRST]), 1);
    p.cpu.set_fs_base(0x5000_0000);
    let before = prepare(&mut p, SET, &[2, 1, 0, 1]);
    assert_eq!(
        stopped(&mut p, before),
        ProcessStop::UnsupportedApi { address: SET }
    );
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, SET, &[2, 1, 0, 2]), FIRST + 4);
    assert_eq!(call(&mut p, REMOVE, &[FIRST + 4]), 1);
}
