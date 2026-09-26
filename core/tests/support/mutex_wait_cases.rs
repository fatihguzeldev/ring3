use std::time::Duration;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

use super::{event_wait_control::*, timed_event_cases::timed_process};

fn finish(p: &mut Process32, teb: u32, value: u32) {
    assert_eq!(call(p, 0x254, &[CURRENT, 0]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), teb);
    assert_eq!(p.cpu.register(Register32::Eax), value);
}

pub fn mutex_release_transfers_recursive_ownership() {
    let mut p = process();
    p.memory.write(u64::from(DATA), b"handoff\0").unwrap();
    let mutex = call(&mut p, 0x210, &[0, 1, DATA]);
    let alias = call(&mut p, 0x210, &[0, 0, DATA]);
    assert_eq!(call(&mut p, 0x214, &[alias, 0]), 0);
    let child = park_child(&mut p, mutex);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
    denied(&mut p, 0x21c, &[mutex]);
    assert_eq!(call(&mut p, 0x218, &[alias]), 1);
    // the partial recursive release must leave the child parked.
    finish(&mut p, PRIMARY, 1);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
    assert_eq!(call(&mut p, 0x218, &[alias]), 1);
    assert_eq!(call(&mut p, 0x214, &[alias, 0]), 258);
    assert_eq!(call(&mut p, 0x218, &[alias]), 0);
    assert_eq!(p.last_error().unwrap(), 288);
    denied(&mut p, 0x21c, &[mutex]);
    assert_eq!(call(&mut p, 0x554, &[child]), 0);
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    assert_eq!(call(&mut p, 0x21c, &[alias]), 1);
    // captured wait arguments survive guest edits before completion.
    put(&mut p, CHILD - 16, 0);
    put(&mut p, CHILD - 12, 0);
    finish(&mut p, CHILD, 0);
    assert_eq!(call(&mut p, 0x214, &[mutex, 0]), 0);
    assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
    assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
    assert_eq!(call(&mut p, 0x218, &[mutex]), 0);
    assert_eq!(call(&mut p, 0x21c, &[mutex]), 1);
}

pub fn mutex_wait_timeout_does_not_acquire_ownership() {
    for early in [true, false] {
        let mut p = timed_process(33);
        let mutex = call(&mut p, 0x210, &[0, 1, 0]);
        park_child(&mut p, mutex);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        p.set_elapsed_time(Duration::from_millis(if early { 32 } else { 33 }))
            .unwrap();
        assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
        assert_eq!(
            call(&mut p, 0x214, &[mutex, 0]),
            if early { 258 } else { 0 }
        );
        finish(&mut p, CHILD, if early { 0 } else { 258 });
        assert_eq!(call(&mut p, 0x218, &[mutex]), u32::from(early));
    }
}

pub fn suspended_mutex_waiters_and_return_faults_preserve_ownership() {
    for competing in [false, true] {
        let mut p = process();
        let mutex = call(&mut p, 0x210, &[0, 1, 0]);
        let first = park_child(&mut p, mutex);
        if competing {
            park_child(&mut p, mutex);
        }
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        assert_eq!(call(&mut p, 0x554, &[first]), 0);
        assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
        if competing {
            finish(&mut p, CHILD + 0x11000, 0);
            assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
            assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
        }
        assert_eq!(call(&mut p, 0x550, &[first]), 1);
        assert_eq!(call(&mut p, 0x214, &[mutex, 0]), 258);
        // a completion read fault must retain both the acquisition and its handle pin.
        p.memory
            .protect(u64::from(CHILD - 4096), 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
        let failed = p.run(100);
        assert!(matches!(
            failed.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((failed.instructions, failed.api_calls), (0, 0));
        let before = p.cpu;
        assert_eq!(p.run(100), failed);
        assert_eq!(p.cpu, before);
        p.memory
            .protect(u64::from(CHILD - 4096), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(call(&mut p, 0x218, &[mutex]), 1);
        assert_eq!(call(&mut p, 0x21c, &[mutex]), 1);
    }
}
