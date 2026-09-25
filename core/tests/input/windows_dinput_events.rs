use super::dinput_acquire_cases;
use super::dinput_event_cases;
use super::dinput_format_cases;
use super::dinput_state_cases;

#[test]
fn imported_buffered_keyboard_events_match_across_budgets() {
    dinput_event_cases::keyboard_events_across_budgets();
}

use dinput_acquire_cases::{DATA, WINDOW};
use dinput_event_cases::{API, COUNT, PROPERTY, buffered, count, read, records, set_buffer};
use dinput_format_cases::FORMAT;
use dinput_state_cases::{DEVICE, OUTPUT, call, process, words};
use ring3_core::execution::Permissions;
use std::time::Duration;

#[test]
fn disabled_and_single_slot_buffers_keep_immediate_state_and_report_overflow() {
    let mut p = buffered(0);
    p.set_keyboard_state([true; 256]).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8004_0207);
    assert_eq!(count(&p), 16);
    assert_eq!(call(&mut p, 0x7000_05c8, &[DEVICE, 256, OUTPUT]), 0);
    let mut immediate = [0; 256];
    p.memory.read(u64::from(OUTPUT), &mut immediate).unwrap();
    assert_eq!(immediate, [0x80; 256]);
    assert_eq!(call(&mut p, 0x7000_0588, &[DEVICE]), 0);
    set_buffer(&mut p, DEVICE, 1);
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 0);
    p.set_keyboard_state([false; 256]).unwrap();
    for _ in 0..2 {
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 1);
        assert_eq!(count(&p), 0);
    }
    assert_eq!(read(&mut p, DEVICE, 0, 0, 0), 1);
    assert_eq!(count(&p), 0);
    assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 0);
}

#[test]
fn overflow_keeps_oldest_events_and_partial_drain_resumes_collection() {
    let mut p = buffered(4);
    let mut keys = [false; 256];
    for key in 0..5 {
        keys[key] = true;
        p.set_keyboard_state(keys).unwrap();
    }
    for _ in 0..2 {
        assert_eq!(read(&mut p, DEVICE, OUTPUT, u32::MAX, 1), 1);
        assert_eq!(count(&p), 3);
        assert_eq!(
            records(&p, OUTPUT, 3),
            [[0, 0x80, 0, 1], [1, 0x80, 0, 2], [2, 0x80, 0, 3]]
        );
    }
    assert_eq!(read(&mut p, DEVICE, 0, 1, 0), 1);
    assert_eq!(count(&p), 1);
    keys[5] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, u32::MAX, 0), 0);
    assert_eq!(
        records(&p, OUTPUT, 3),
        [[1, 0x80, 0, 2], [2, 0x80, 0, 3], [5, 0x80, 0, 6]]
    );
    assert_eq!(read(&mut p, DEVICE, 0x5000_0000, u32::MAX, 0), 0);
    assert_eq!(count(&p), 0);
}

#[test]
fn buffer_cap_and_null_peek_or_flush_bound_arbitrary_requested_counts() {
    for capacity in [16, 1024, 1025, u32::MAX] {
        let mut p = buffered(capacity);
        for pressed in [true, false, true, false, true] {
            p.set_keyboard_state([pressed; 256]).unwrap();
        }
        let expected = capacity.min(1024) - 1;
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 1);
        assert_eq!(count(&p), expected);
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 0), 1);
        assert_eq!(count(&p), expected);
        assert_eq!(read(&mut p, DEVICE, 0, u32::MAX, 1), 0);
        assert_eq!(count(&p), 0);
    }
}

#[test]
fn devices_share_sample_identity_but_have_independent_drains_and_overflow() {
    let mut p = buffered(2);
    let second = DEVICE + 4;
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    assert_eq!(call(&mut p, 0x7000_0578, &[second, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_057c, &[second, WINDOW, 6]), 0);
    set_buffer(&mut p, second, 16);
    assert_eq!(call(&mut p, 0x7000_0584, &[second]), 0);
    p.set_elapsed_time(Duration::from_millis(55)).unwrap();
    let mut keys = [false; 256];
    keys[4] = true;
    keys[5] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 1);
    assert_eq!(records(&p, OUTPUT, 1), [[4, 0x80, 55, 1]]);
    assert_eq!(read(&mut p, second, OUTPUT, 16, 0), 0);
    assert_eq!(records(&p, OUTPUT, 2), [[4, 0x80, 55, 1], [5, 0x80, 55, 1]]);
    assert_eq!(read(&mut p, DEVICE, 0, 16, 0), 0);
    assert_eq!(count(&p), 0);
}

#[test]
fn acquisition_and_configuration_lifetimes_never_replay_stale_events() {
    let mut p = buffered(16);
    let mut keys = [false; 256];
    keys[1] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 1);
    assert_eq!(read(&mut p, DEVICE, 0, 16, 1), 0);
    assert_eq!(count(&p), 1);
    words(&mut p, PROPERTY, &[20, 16, 0, 0, 9]);
    assert_eq!(
        call(&mut p, 0x7000_0580, &[DEVICE, 1, PROPERTY]),
        0x8007_00aa
    );
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0x8007_00aa);
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 6]), 0x8007_00aa);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 1), 0);
    assert_eq!(records(&p, OUTPUT, 1), [[1, 0x80, 0, 1]]);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    keys[2] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8007_001e);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0x8007_001e);
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    p.set_keyboard_state([false; 256]).unwrap();
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 1), 0);
    assert_eq!(count(&p), 2);
    assert_eq!(call(&mut p, 0x7000_0588, &[DEVICE]), 0);
    for api in [0x7000_0580, 0x7000_0578, 0x7000_057c] {
        let args: &[u32] = match api {
            0x7000_0580 => &[DEVICE, 1, PROPERTY],
            0x7000_0578 => &[DEVICE, FORMAT],
            _ => &[DEVICE, WINDOW, 6],
        };
        assert_eq!(call(&mut p, api, args), 0);
    }
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 0);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    keys.fill(false);
    keys[1] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(read(&mut p, DEVICE, 0, 16, 1), 0);
    assert_eq!(count(&p), 1);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
    let replacement = DEVICE + 4;
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    assert_eq!(call(&mut p, 0x7000_0578, &[replacement, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_057c, &[replacement, WINDOW, 6]), 0);
    set_buffer(&mut p, replacement, 16);
    assert_eq!(call(&mut p, 0x7000_0584, &[replacement]), 0);
    assert_eq!(read(&mut p, replacement, OUTPUT, 16, 0), 0);
    assert_eq!(count(&p), 0);
    p.set_keyboard_state([false; 256]).unwrap();
    assert_eq!(read(&mut p, replacement, OUTPUT, 16, 0), 0);
    assert_eq!(records(&p, OUTPUT, 1), [[1, 0, 0, 5]]);
}

#[test]
fn timestamp_wrap_uses_same_clock_as_time_get_time() {
    let mut p = buffered(16);
    p.set_elapsed_time(Duration::from_millis(u64::from(u32::MAX)))
        .unwrap();
    let mut keys = [false; 256];
    keys[255] = true;
    p.set_keyboard_state(keys).unwrap();
    p.set_elapsed_time(Duration::from_millis(u64::from(u32::MAX) + 2))
        .unwrap();
    keys[255] = false;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(call(&mut p, 0x7000_0244, &[]), 1);
    assert_eq!(read(&mut p, DEVICE, OUTPUT, 16, 0), 0);
    assert_eq!(
        records(&p, OUTPUT, 2),
        [[255, 0x80, u32::MAX, 1], [255, 0, 1, 2]]
    );
}

#[test]
fn hresult_errors_preserve_count_records_and_real_error_storage() {
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
