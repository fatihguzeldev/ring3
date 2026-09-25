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
mod dinput_mouse_event_cases;

#[test]
fn imported_buffered_mouse_events_match_across_budgets() {
    dinput_mouse_event_cases::mouse_events_across_budgets();
}

use dinput_acquire_cases::{DATA, WINDOW};
use dinput_event_cases::{COUNT, PROPERTY, count, records};
use dinput_mouse_event_cases::{API, buffered, read, set_buffer};
use dinput_mouse_format_cases::FORMAT;
use dinput_mouse_state_cases::{DEVICE, process, state};
use dinput_state_cases::{OUTPUT, call, words};
use ring3_core::execution::{MouseInput, MouseInputError, Permissions};
use std::time::Duration;

fn movement(x: i32) -> MouseInput {
    MouseInput {
        relative: [x, 0],
        ..MouseInput::default()
    }
}

#[test]
fn keyboard_and_mouse_share_clock_and_sequence_without_sharing_drains() {
    let mut p = buffered(16);
    let keyboard = dinput_state_cases::DEVICE;
    p.memory
        .write(
            u64::from(DATA + 64),
            &[
                0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0, 0,
            ],
        )
        .unwrap();
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    p.memory
        .protect(0x3000_0000, 12288, Permissions::READ_WRITE)
        .unwrap();
    dinput_format_cases::standard(&mut p.memory, false);
    p.memory
        .protect(0x3000_0000, 12288, Permissions::READ)
        .unwrap();
    assert_eq!(
        call(
            &mut p,
            0x7000_0578,
            &[keyboard, dinput_format_cases::FORMAT]
        ),
        0
    );
    assert_eq!(call(&mut p, 0x7000_057c, &[keyboard, WINDOW, 6]), 0);
    dinput_event_cases::set_buffer(&mut p, keyboard, 16);
    assert_eq!(call(&mut p, 0x7000_0584, &[keyboard]), 0);
    p.set_elapsed_time(Duration::from_millis(u64::from(u32::MAX)))
        .unwrap();
    let mut keys = [false; 256];
    keys[30] = true;
    p.set_keyboard_state(keys).unwrap();
    p.submit_mouse_input(movement(-2)).unwrap();
    p.submit_mouse_input(MouseInput::default()).unwrap();
    p.set_keyboard_state(keys).unwrap();
    p.set_elapsed_time(Duration::from_millis(u64::from(u32::MAX) + 2))
        .unwrap();
    p.set_keyboard_state([false; 256]).unwrap();
    p.submit_mouse_input(movement(3)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(
        records(&p, OUTPUT, 2),
        [[0, (-2_i32).cast_unsigned(), u32::MAX, 2], [0, 3, 1, 4]]
    );
    assert_eq!(dinput_event_cases::read(&mut p, keyboard, OUTPUT, 16, 0), 0);
    assert_eq!(
        records(&p, OUTPUT, 2),
        [[30, 128, u32::MAX, 1], [30, 0, 1, 3]]
    );
    assert_eq!(call(&mut p, 0x7000_0244, &[]), 1);
}

#[test]
fn rejected_host_samples_preserve_events_sequence_motion_and_buttons() {
    let mut p = buffered(16);
    p.submit_mouse_input(movement(i32::MAX)).unwrap();
    for input in [
        MouseInput {
            relative: [1, 4],
            buttons: [true; 8],
            ..MouseInput::default()
        },
        MouseInput {
            wheel_steps: i32::MAX,
            buttons: [true; 8],
            ..MouseInput::default()
        },
    ] {
        let cpu = p.cpu;
        assert_eq!(
            p.submit_mouse_input(input),
            Err(MouseInputError::MotionOverflow)
        );
        assert_eq!(p.cpu, cpu);
    }
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 1), 0);
    assert_eq!(count(&p), 1);
    assert_eq!(
        records(&p, OUTPUT, 1),
        [[0, i32::MAX.cast_unsigned(), 0, 1]]
    );
    assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT + 128]), 0);
    assert_eq!(state(&p, OUTPUT + 128), ([i32::MAX, 0, 0], [0; 8]));
    p.submit_mouse_input(MouseInput {
        buttons: [true; 8],
        ..MouseInput::default()
    })
    .unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 9);
    for (index, record) in records(&p, OUTPUT + 16, 8).iter().enumerate() {
        assert_eq!(*record, [12 + u32::try_from(index).unwrap(), 128, 0, 2]);
    }
}

#[test]
fn overflow_and_disabled_buffers_preserve_independent_immediate_motion() {
    let mut p = buffered(0);
    p.submit_mouse_input(movement(3)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8004_0207);
    assert_eq!(count(&p), 16);
    assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT]), 0);
    assert_eq!(state(&p, OUTPUT), ([3, 0, 0], [0; 8]));
    assert_eq!(call(&mut p, 0x7000_05b0, &[DEVICE]), 0);
    set_buffer(&mut p, DEVICE, 4);
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 0);
    for value in 1..=5 {
        p.submit_mouse_input(movement(value)).unwrap();
    }
    for _ in 0..2 {
        assert_eq!(read(&mut p, DEVICE, OUTPUT, u32::MAX, 1), 1);
        assert_eq!(count(&p), 3);
        assert_eq!(
            records(&p, OUTPUT, 3),
            [[0, 1, 0, 2], [0, 2, 0, 3], [0, 3, 0, 4]]
        );
    }
    assert_eq!(read(&mut p, DEVICE, 0, 1, 0), 1);
    p.submit_mouse_input(movement(6)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(
        records(&p, OUTPUT, 3),
        [[0, 2, 0, 3], [0, 3, 0, 4], [0, 6, 0, 7]]
    );
    assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT]), 0);
    assert_eq!(state(&p, OUTPUT), ([21, 0, 0], [0; 8]));
}

#[test]
fn bounded_mouse_capacity_and_null_reads_acknowledge_overflow() {
    for capacity in [1, 16, 1024, 1025, u32::MAX] {
        let mut p = buffered(capacity);
        for _ in 0..1024 {
            p.submit_mouse_input(movement(1)).unwrap();
        }
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 1);
        assert_eq!(count(&p), capacity.min(1024) - 1);
        assert_eq!(read(&mut p, DEVICE, 0x5000_0000, 0, 0), 1);
        assert_eq!(count(&p), 0);
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 0), 0);
        assert_eq!(count(&p), capacity.min(1024) - 1);
        assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
        assert_eq!(count(&p), 0);
    }
}

#[test]
fn acquisition_focus_and_rejected_setters_preserve_or_reset_pending_history() {
    let mut p = buffered(16);
    p.submit_mouse_input(movement(7)).unwrap();
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 1);
    words(&mut p, PROPERTY, &[20, 16, 0, 0, 9]);
    assert_eq!(
        call(&mut p, 0x7000_05a4, &[DEVICE, 1, PROPERTY]),
        0x8007_00aa
    );
    assert_eq!(call(&mut p, 0x7000_0598, &[DEVICE, FORMAT]), 0x8007_00aa);
    assert_eq!(call(&mut p, 0x7000_059c, &[DEVICE, WINDOW, 5]), 0x8007_00aa);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 1), 0);
    assert_eq!(records(&p, OUTPUT, 1), [[0, 7, 0, 1]]);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    p.submit_mouse_input(movement(8)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8007_001e);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8007_001e);
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    p.submit_mouse_input(movement(9)).unwrap();
    assert_eq!(call(&mut p, 0x7000_05b0, &[DEVICE]), 0);
    p.submit_mouse_input(movement(10)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8007_000c);
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    p.submit_mouse_input(movement(11)).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(records(&p, OUTPUT, 1), [[0, 11, 0, 5]]);
}

#[test]
fn final_release_transfers_exclusive_collection_without_replaying_old_events() {
    let mut p = buffered(16);
    let second = DEVICE + 4;
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    assert_eq!(call(&mut p, 0x7000_0598, &[second, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_059c, &[second, WINDOW, 5]), 0);
    set_buffer(&mut p, second, 16);
    assert_eq!(call(&mut p, 0x7000_05ac, &[second]), 0x8007_0005);
    p.submit_mouse_input(movement(1)).unwrap();
    assert_eq!(read(&mut p, DEVICE, 0, 16, 1), 0);
    assert_eq!(count(&p), 1);
    assert_eq!(call(&mut p, 0x7000_0594, &[DEVICE]), 0);
    p.submit_mouse_input(movement(2)).unwrap();
    assert_eq!(call(&mut p, 0x7000_05ac, &[second]), 0);
    assert_eq!(read(&mut p, second, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    p.submit_mouse_input(movement(3)).unwrap();
    assert_eq!(read(&mut p, second, OUTPUT, 16, 0), 0);
    assert_eq!(records(&p, OUTPUT, 1), [[0, 3, 0, 3]]);
}

#[test]
fn mouse_event_hresult_priority_preserves_buffers_and_real_error_storage() {
    let mut p = process(false);
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7000_2020, &[88]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    words(&mut p, COUNT, &[9]);
    for (size, flags, pointer) in [(0, 0, COUNT), (20, 0, COUNT), (16, 2, COUNT), (16, 0, 0)] {
        assert_eq!(
            call(&mut p, API, &[DEVICE, size, 0x5000_0000, pointer, flags]),
            0x8007_0057
        );
    }
    assert_eq!(
        call(&mut p, API, &[DEVICE, 16, 0, 0x5000_0000, 0]),
        0x8007_000c
    );
    assert_eq!(count(&p), 9);
    assert_eq!(records(&p, OUTPUT, 1), [[0; 4]]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}
