use std::time::Duration;

use ring3_core::execution::{
    MouseInput, Permissions, Process32, ProcessStop, Register32, StopReason,
};

use super::dinput_event_cases::{COUNT, PROPERTY, count, records};
use super::dinput_mouse_state_cases::{DEVICE, process, state};
use super::dinput_state_cases::{CODE, OUTPUT, STACK, call, words};

pub const API: u32 = 0x7000_05d4;

pub fn set_buffer(p: &mut Process32, device: u32, capacity: u32) {
    words(p, PROPERTY, &[20, 16, 0, 0, capacity]);
    assert_eq!(call(p, 0x7000_05a4, &[device, 1, PROPERTY]), 0);
}

pub fn buffered(capacity: u32) -> Process32 {
    let mut p = process(true);
    set_buffer(&mut p, DEVICE, capacity);
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 0);
    p
}

pub fn read(p: &mut Process32, device: u32, output: u32, requested: u32, flags: u32) -> u32 {
    words(p, COUNT, &[requested]);
    call(p, API, &[device, 16, output, COUNT, flags])
}

fn append_read(code: &mut Vec<u8>, output: u32, flags: u32) {
    for value in [flags, COUNT, output, 16] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 40, 0xcc]);
}

fn phase(p: &mut Process32, budget: u64) -> (u64, u64) {
    let mut counts = (0, 0);
    loop {
        let result = p.run(budget);
        counts.0 += result.instructions;
        counts.1 += result.api_calls;
        assert!(counts.0 < 1000);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(p.cpu.register(Register32::Esp), STACK);
    counts
}

pub fn mouse_events_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = buffered(16);
        let mut code = vec![0xbb];
        code.extend_from_slice(&DEVICE.to_le_bytes());
        code.extend_from_slice(&[0x8b, 0x2b]);
        for (index, flags) in [(0, 1), (1, 0), (2, 0)] {
            append_read(&mut code, OUTPUT + index * 128, flags);
        }
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(u64::from(CODE), &code).unwrap();
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        p.set_elapsed_time(Duration::from_millis(1234)).unwrap();
        let buttons = [true, false, false, false, false, false, false, true];
        p.submit_mouse_input(MouseInput {
            relative: [4, -6],
            wheel_steps: 2,
            buttons,
        })
        .unwrap();
        p.submit_mouse_input(MouseInput {
            buttons,
            ..MouseInput::default()
        })
        .unwrap();
        assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT + 512]), 0);
        assert_eq!(
            state(&p, OUTPUT + 512),
            ([4, -6, 240], [128, 0, 0, 0, 0, 0, 0, 128])
        );
        p.cpu.eip = CODE;
        p.cpu.set_register(Register32::Esp, STACK);
        words(&mut p, COUNT, &[16]);
        let first = phase(&mut p, budget);
        let expected = [
            [0, 4, 1234, 1],
            [4, (-6_i32).cast_unsigned(), 1234, 1],
            [8, 240, 1234, 1],
            [12, 128, 1234, 1],
            [19, 128, 1234, 1],
        ];
        assert_eq!(count(&p), 5);
        assert_eq!(records(&p, OUTPUT, 5), expected);
        words(&mut p, COUNT, &[2]);
        let second = phase(&mut p, budget);
        assert_eq!(count(&p), 2);
        assert_eq!(records(&p, OUTPUT + 128, 2), expected[..2]);
        p.set_elapsed_time(Duration::from_secs(2)).unwrap();
        p.submit_mouse_input(MouseInput {
            relative: [-3, 0],
            wheel_steps: -1,
            buttons: [false; 8],
        })
        .unwrap();
        words(&mut p, COUNT, &[u32::MAX]);
        let third = phase(&mut p, budget);
        assert_eq!(count(&p), 7);
        let last = records(&p, OUTPUT + 256, 7);
        assert_eq!(
            last,
            [
                expected[2],
                expected[3],
                expected[4],
                [0, (-3_i32).cast_unsigned(), 2000, 2],
                [8, (-120_i32).cast_unsigned(), 2000, 2],
                [12, 0, 2000, 2],
                [19, 0, 2000, 2],
            ]
        );
        assert_eq!(call(&mut p, 0x7000_05d0, &[DEVICE, 20, OUTPUT + 512]), 0);
        assert_eq!(state(&p, OUTPUT + 512), ([-3, 0, -120], [0; 8]));
        assert_eq!(read(&mut p, DEVICE, 0x5000_0000, 16, 0), 0);
        assert_eq!(count(&p), 0);
        results.push((p.cpu, first, second, third, last, p.memory.mapped_pages()));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
