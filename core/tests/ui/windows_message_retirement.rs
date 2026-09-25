use super::window_creation_executable;
use super::window_message_cases;
use super::window_message_retirement_cases;

#[test]
fn retired_parent_and_child_messages_disappear_across_budgets() {
    window_message_retirement_cases::verify();
}

use ring3_core::execution::{
    Permissions, PostMessageError, Process32, ProcessStop, Register32, StopReason,
};
use window_message_cases::{STACK, WINDOW, call, prepare};
use window_message_retirement_cases::{
    CHILD, DESTROY, DIALOG, OUTPUT, PROCEDURE, created, finish, message, peek,
};

fn write_code(p: &mut Process32, address: u32, code: &[u8]) {
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(address), code).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
}

#[test]
fn retirement_reclaims_capacity_and_keeps_quit_notifications_and_thread_order() {
    let mut p = created();
    p.post_message(message(0, 0x12)).unwrap();
    p.post_message(message(0, 0x401)).unwrap();
    for index in 0..1022 {
        p.post_message(message(if index % 2 == 0 { DIALOG } else { CHILD }, 0))
            .unwrap();
    }
    assert_eq!(
        p.post_message(message(0, 0x402)),
        Err(PostMessageError::Full)
    );
    prepare(&mut p, DESTROY, &[DIALOG]);
    finish(&mut p, 100);
    for _ in 0..1022 {
        p.post_message(message(0, 0x402)).unwrap();
    }
    assert_eq!(
        p.post_message(message(0, 0x403)),
        Err(PostMessageError::Full)
    );
    assert_eq!(peek(&mut p, true).unwrap()[..2], [0, 0x401]);
    for _ in 0..1022 {
        assert_eq!(peek(&mut p, true).unwrap()[..2], [0, 0x402]);
    }
    assert_eq!(
        peek(&mut p, true).unwrap(),
        [0, 0x12, 19, 23, 77, (-2_i32).cast_unsigned(), 3, 0]
    );
    assert_eq!(peek(&mut p, true), None);
}

#[test]
fn callback_transition_faults_preserve_windows_and_queue_until_successful_retry() {
    let mut p = created();
    for _ in 0..1024 {
        p.post_message(message(CHILD, 0)).unwrap();
    }
    prepare(&mut p, DESTROY, &[DIALOG]);
    assert_eq!(
        p.run(3).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.eip, 0x7000_0ff8);
    assert_eq!(p.cpu.register(Register32::Esp), STACK);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ)
        .unwrap();
    let before = p.cpu;
    let windows = p.window_snapshots();
    for _ in 0..2 {
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(p.window_snapshots(), windows);
        assert_eq!(
            p.post_message(message(0, 0x401)),
            Err(PostMessageError::Full)
        );
    }
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.window_snapshots().len(), 2);
    assert_eq!(p.cpu.eip, PROCEDURE + 5);
    p.post_message(message(0, 0x401)).unwrap();
    for _ in 0..1023 {
        p.post_message(message(DIALOG, 0)).unwrap();
    }
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.eip, 0x7000_0ff8);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::NONE)
        .unwrap();
    let before = p.cpu;
    let windows = p.window_snapshots();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(
        p.post_message(message(0, 0x402)),
        Err(PostMessageError::Full)
    );
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    finish(&mut p, 100);
    p.post_message(message(0, 0x402)).unwrap();
    assert_eq!(peek(&mut p, true).unwrap()[..2], [0, 0x401]);
    assert_eq!(peek(&mut p, true).unwrap()[..2], [0, 0x402]);
    assert_eq!(peek(&mut p, true), None);
}

#[test]
fn hiding_ending_or_failed_destruction_does_not_discard_live_messages() {
    let mut p = created();
    p.post_message(message(DIALOG, 0)).unwrap();
    p.post_message(message(CHILD, 0x401)).unwrap();
    call(&mut p, 0x7000_0440, &[DIALOG, 0]);
    assert_eq!(call(&mut p, 0x7000_0468, &[DIALOG, 1]), 1);
    assert_eq!(call(&mut p, DESTROY, &[0]), 0);
    let before = prepare(&mut p, DESTROY, &[WINDOW]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: DESTROY }
    );
    assert_eq!(p.cpu, before);
    let before = prepare(&mut p, DESTROY, &[DIALOG]);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ)
        .unwrap();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.window_snapshots().len(), 3);
    assert_eq!(peek(&mut p, true).unwrap()[..2], [DIALOG, 0]);
    assert_eq!(peek(&mut p, true).unwrap()[..2], [CHILD, 0x401]);
}

#[test]
fn messages_posted_by_destroy_callbacks_are_purged_but_guest_copies_remain() {
    let mut p = created();
    p.post_message(message(DIALOG, 0x400)).unwrap();
    let copied = peek(&mut p, true).unwrap();
    write_code(
        &mut p,
        PROCEDURE,
        &[
            0x6a, 0, 0x6a, 0, 0x68, 5, 4, 0, 0, 0xff, 0x74, 0x24, 16, 0xb8, 0x64, 4, 0, 0x70, 0xff,
            0xd0, 0xb8, 1, 0, 0, 0, 0xc2, 16, 0,
        ],
    );
    prepare(&mut p, DESTROY, &[DIALOG]);
    finish(&mut p, 100);
    let mut retained = [0; 32];
    p.memory.read(u64::from(OUTPUT), &mut retained).unwrap();
    assert_eq!(retained.as_slice(), copied.map(u32::to_le_bytes).concat());
    let before = prepare(&mut p, 0x7000_0450, &[OUTPUT]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi {
            address: 0x7000_0450
        }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(peek(&mut p, true), None);
}

#[test]
fn rejected_window_creation_discards_messages_posted_from_its_procedure() {
    let mut procedure = vec![0x81, 0x7c, 0x24, 8, 0x81, 0, 0, 0, 0x75, 22];
    procedure.extend_from_slice(&[
        0x6a, 0, 0x6a, 0, 0x6a, 0, 0xff, 0x74, 0x24, 16, 0xb8, 0x64, 4, 0, 0x70, 0xff, 0xd0, 0x31,
        0xc0, 0xc2, 16, 0,
    ]);
    procedure.extend(window_creation_executable::default_procedure());
    let mut p = Process32::load(&window_creation_executable::pe32(&procedure), 32).unwrap();
    finish(&mut p, 100);
    assert!(p.window_snapshots().is_empty());
    assert_eq!(p.cpu.register(Register32::Ebx), 0);
    assert_eq!(peek(&mut p, true), None);
}

#[test]
fn rejected_dialog_hook_discards_parent_and_child_messages_without_touching_older_windows() {
    let mut p = created();
    let hook = 0x0040_11c0;
    write_code(&mut p, hook, &[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    assert_ne!(call(&mut p, 0x7000_024c, &[5, hook, 0, 1]), 0);
    prepare(
        &mut p,
        0x7000_0434,
        &[0x0040_0000, 0x1000_c000, 0, PROCEDURE, 0],
    );
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.eip, hook);
    assert_eq!(p.window_snapshots().len(), 5);
    p.post_message(message(DIALOG + 8, 0)).unwrap();
    p.post_message(message(CHILD + 8, 0x401)).unwrap();
    p.post_message(message(DIALOG, 0x402)).unwrap();
    finish(&mut p, 100);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(p.window_snapshots().len(), 3);
    assert_eq!(peek(&mut p, true).unwrap()[..2], [DIALOG, 0x402]);
    assert_eq!(peek(&mut p, true), None);
}
