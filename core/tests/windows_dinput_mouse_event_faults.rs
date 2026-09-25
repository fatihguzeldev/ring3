#[path = "support/dinput_acquire_cases.rs"]
#[allow(dead_code)]
mod dinput_acquire_cases;
#[path = "support/dinput_cooperative_cases.rs"]
#[allow(dead_code)]
mod dinput_cooperative_cases;
#[path = "support/dinput_device_cases.rs"]
#[allow(dead_code)]
mod dinput_device_cases;
#[path = "support/dinput_format_cases.rs"]
#[allow(dead_code)]
mod dinput_format_cases;
#[path = "support/dinput_mouse_acquire_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_acquire_cases;
#[path = "support/dinput_mouse_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_cases;
#[path = "support/dinput_mouse_format_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_format_cases;
#[path = "support/dinput_mouse_state_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_state_cases;
#[path = "support/dinput_state_cases.rs"]
#[allow(dead_code)]
mod dinput_state_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[path = "support/dinput_event_cases.rs"]
#[allow(dead_code)]
mod dinput_event_cases;
#[path = "support/dinput_mouse_event_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_event_cases;

use dinput_event_cases::{COUNT, count, records};
use dinput_mouse_event_cases::{API, buffered, read};
use dinput_mouse_state_cases::{DEVICE, state};
use dinput_state_cases::{CODE, OUTPUT, STACK, call, prepare, words};
use ring3_core::execution::{
    MouseInput, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn pending() -> Process32 {
    let mut p = buffered(2);
    p.submit_mouse_input(MouseInput {
        relative: [7, 8],
        ..MouseInput::default()
    })
    .unwrap();
    p
}

fn retained(p: &mut Process32) {
    assert_eq!(read(p, DEVICE, OUTPUT, 16, 1), 1);
    assert_eq!(count(p), 1);
    assert_eq!(records(p, OUTPUT, 1), [[0, 7, 0, 1]]);
}

#[test]
fn count_and_data_faults_preserve_mouse_queue_overflow_and_immediate_motion() {
    let mut p = pending();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0x3200_0000, 4096, Permissions::READ)
        .unwrap();
    for (output, pointer) in [
        (OUTPUT, 0x5000_0000),
        (OUTPUT, 0xffff_fffe),
        (OUTPUT, 0x3200_0000),
        (0xffff_fff8, COUNT),
        (0x3100_0ff9, COUNT),
        (0x3200_0000, COUNT),
    ] {
        words(&mut p, COUNT, &[1]);
        p.memory.write(0x3100_0ff9, &[0x55; 7]).unwrap();
        let before = prepare(&mut p, API, &[DEVICE, 16, output, pointer, 0]);
        for _ in 0..2 {
            let result = p.run(1);
            assert!(matches!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
        let mut prefix = [0; 7];
        p.memory.read(0x3100_0ff9, &mut prefix).unwrap();
        assert_eq!(prefix, [0x55; 7]);
        assert_eq!(count(&p), 1);
        retained(&mut p);
    }
    words(&mut p, COUNT, &[1]);
    prepare(&mut p, API, &[DEVICE, 16, 0x3100_0ff9, COUNT, 0]);
    p.memory
        .map_zeroed(0x3100_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(records(&p, 0x3100_0ff9, 1), [[0, 7, 0, 1]]);
    assert_eq!(read(&mut p, DEVICE, 0, 16, 1), 0);
    assert_eq!(count(&p), 0);
    assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT]), 0);
    assert_eq!(state(&p, OUTPUT), ([7, 8, 0], [0; 8]));
}

#[test]
fn mouse_data_count_and_return_aliases_follow_publication_order() {
    for (output, pointer, expected_return) in [
        (OUTPUT, OUTPUT + 4, CODE),
        (STACK, COUNT, 0),
        (STACK, STACK + 4, 0),
        (STACK, STACK, 1),
        (0x7ffd_e034, COUNT, CODE),
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
        let mut expected = [0, 7, 0, 1];
        if pointer == output {
            expected[0] = 1;
        }
        if pointer == output + 4 {
            expected[1] = 1;
        }
        assert_eq!(records(&p, output, 1), [expected]);
        assert_eq!(read(&mut p, DEVICE, 0, 16, 1), 0);
        assert_eq!(count(&p), 0);
    }
    let mut p = pending();
    p.memory
        .map_zeroed(
            0x3200_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(read(&mut p, DEVICE, 0x3200_0001, 16, 0), 1);
    p.memory
        .protect(0x3200_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(records(&p, 0x3200_0001, 1), [[0, 7, 0, 1]]);
}

#[test]
fn receiver_actor_and_whole_frame_guards_preserve_pending_mouse_events() {
    let mut p = pending();
    for receiver in [
        0,
        DEVICE + 1,
        DEVICE + 4,
        0x7001_7800,
        0x7001_7900,
        u32::MAX,
    ] {
        let before = prepare(&mut p, API, &[receiver, 0, 0, 0, 0]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
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
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_ffe8, &[CODE, DEVICE, 16, OUTPUT, COUNT, 0]);
    for stack in [0x1000_ffec, 0xffff_ffe8] {
        p.cpu.eip = API;
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    retained(&mut p);
}
