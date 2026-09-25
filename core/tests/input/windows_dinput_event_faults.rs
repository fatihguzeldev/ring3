use super::dinput_event_cases;
use super::dinput_state_cases;

use dinput_event_cases::{API, COUNT, buffered, count, read, records};
use dinput_state_cases::{CODE, DEVICE, OUTPUT, STACK, call, prepare, words};
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn pending() -> Process32 {
    let mut p = buffered(2);
    let mut keys = [false; 256];
    keys[7] = true;
    keys[8] = true;
    p.set_keyboard_state(keys).unwrap();
    p
}

fn refused(p: &mut Process32, expected: Option<&MemoryError>) {
    let before = p.cpu;
    for _ in 0..2 {
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        if let Some(error) = expected {
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(error.clone()))
            );
        }
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

fn retained(p: &mut Process32) {
    assert_eq!(read(p, DEVICE, OUTPUT, 16, 1), 1);
    assert_eq!(count(p), 1);
    assert_eq!(records(p, OUTPUT, 1), [[7, 0x80, 0, 1]]);
}

#[test]
fn count_and_record_faults_preserve_the_entire_event_and_overflow_for_retry() {
    let mut p = pending();
    words(&mut p, COUNT, &[1]);
    p.memory.write(u64::from(OUTPUT), &[0x55; 16]).unwrap();
    prepare(&mut p, API, &[DEVICE, 16, OUTPUT, COUNT, 0]);
    p.memory
        .protect(0x3100_0000, 4096, Permissions::READ)
        .unwrap();
    refused(&mut p, None);
    assert_eq!(count(&p), 1);
    assert_eq!(records(&p, OUTPUT, 1), [[0x5555_5555; 4]]);
    p.memory
        .protect(0x3100_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    retained(&mut p);
    p.memory.write(0x3100_0ff9, &[0x55; 7]).unwrap();
    prepare(&mut p, API, &[DEVICE, 16, 0x3100_0ff9, COUNT, 0]);
    refused(
        &mut p,
        Some(&MemoryError::Unmapped {
            address: 0x3100_1000,
        }),
    );
    let mut prefix = [0; 7];
    p.memory.read(0x3100_0ff9, &mut prefix).unwrap();
    assert_eq!(prefix, [0x55; 7]);
    assert_eq!(count(&p), 1);
    p.memory
        .map_zeroed(0x3100_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(records(&p, 0x3100_0ff9, 1), [[7, 0x80, 0, 1]]);
    assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 0);
    assert_eq!(count(&p), 0);
}

#[test]
fn wrapped_count_or_data_spans_and_read_only_data_do_not_consume() {
    let mut p = pending();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (output, pointer) in [(OUTPUT, 0xffff_fffe), (0xffff_fff8, COUNT)] {
        words(&mut p, COUNT, &[1]);
        prepare(&mut p, API, &[DEVICE, 16, output, pointer, 0]);
        refused(&mut p, Some(&MemoryError::AddressOverflow));
        retained(&mut p);
    }
    p.memory
        .map_zeroed(0x3200_0000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, &[DEVICE, 16, 0x3200_0000, COUNT, 0]);
    refused(&mut p, None);
    retained(&mut p);
    p.memory
        .protect(
            0x3200_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(read(&mut p, DEVICE, 0x3200_0001, 1, 0), 1);
    p.memory
        .protect(0x3200_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(records(&p, 0x3200_0001, 1), [[7, 0x80, 0, 1]]);
}

#[test]
fn count_and_return_aliases_follow_data_then_count_publication() {
    for (output, pointer, expected_return) in [
        (OUTPUT, OUTPUT + 4, CODE),
        (STACK, COUNT, 7),
        (STACK, STACK + 4, 7),
        (STACK, STACK, 1),
    ] {
        let mut p = pending();
        prepare(&mut p, API, &[DEVICE, 16, output, pointer, 0]);
        if pointer != STACK && pointer != STACK + 4 {
            words(&mut p, pointer, &[1]);
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(p.cpu.eip, expected_return);
        assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        let mut expected = [7, 0x80, 0, 1];
        if pointer == output {
            expected[0] = 1;
        }
        if pointer == output + 4 {
            expected[1] = 1;
        }
        assert_eq!(records(&p, output, 1), [expected]);
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 0);
        assert_eq!(count(&p), 0);
    }
}

#[test]
fn identity_actor_and_full_frame_guards_cannot_consume_pending_events() {
    let mut p = pending();
    for receiver in [
        0,
        DEVICE + 1,
        DEVICE + 4,
        0x7001_7800,
        0x7001_7a00,
        u32::MAX,
    ] {
        prepare(&mut p, API, &[receiver, 0, 0, 0, 0]);
        let before = p.cpu;
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!(p.cpu, before);
    }
    for fs in [0, 0x1101_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = API;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        let before = p.cpu;
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(p.cpu, before);
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    let before = prepare(&mut p, API, &[DEVICE, 16, OUTPUT, COUNT, 0]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    words(&mut p, 0x1000_ffec, &[CODE, DEVICE, 16, OUTPUT, COUNT]);
    p.cpu.set_register(Register32::Esp, 0x1000_ffec);
    refused(
        &mut p,
        Some(&MemoryError::Unmapped {
            address: 0x1001_0000,
        }),
    );
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_ffe8, &[CODE, DEVICE, 16, OUTPUT, COUNT, 0]);
    p.cpu.set_register(Register32::Esp, 0xffff_ffe8);
    refused(&mut p, Some(&MemoryError::AddressOverflow));
    retained(&mut p);
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 1);
    retained(&mut p);
}
