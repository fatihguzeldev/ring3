use ring3_core::execution::{
    MouseInput, Permissions, Process32, ProcessStop, Register32, StopReason,
};

use super::dinput_state_cases::{CODE, OUTPUT, STACK, call};
use super::{dinput_acquire_cases, dinput_mouse_acquire_cases, dinput_mouse_format_cases};

pub const DEVICE: u32 = 0x7001_7a00;
pub const API: u32 = 0x7000_05d0;

pub fn process(configured: bool) -> Process32 {
    let mut p = dinput_mouse_acquire_cases::process();
    let data = dinput_acquire_cases::DATA;
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, data, 0]), 0);
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, data + 64, data, 0]),
        0
    );
    if configured {
        assert_eq!(
            call(
                &mut p,
                0x7000_0598,
                &[DEVICE, dinput_mouse_format_cases::FORMAT]
            ),
            0
        );
        assert_eq!(
            call(
                &mut p,
                0x7000_059c,
                &[DEVICE, dinput_acquire_cases::WINDOW, 5]
            ),
            0
        );
    }
    p.memory
        .map_zeroed(0x3100_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p
}

pub fn acquired() -> Process32 {
    let mut p = process(true);
    assert_eq!(call(&mut p, 0x7000_05ac, &[DEVICE]), 0);
    p
}

pub fn state(p: &Process32, address: u32) -> ([i32; 3], [u8; 8]) {
    let mut bytes = [0; 20];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    let axes =
        std::array::from_fn(|i| i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    (axes, bytes[12..].try_into().unwrap())
}

fn append_read(code: &mut Vec<u8>, output: u32) {
    for value in [output, 20] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 36]);
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
    assert_eq!(p.cpu.register(Register32::Esp), STACK);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    counts
}

pub fn mouse_state_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = acquired();
        let mut code = vec![0xbb];
        code.extend_from_slice(&DEVICE.to_le_bytes());
        code.extend_from_slice(&[0x8b, 0x2b, 0x53, 0xff, 0x55, 28]);
        append_read(&mut code, OUTPUT);
        append_read(&mut code, OUTPUT + 32);
        code.push(0xcc);
        append_read(&mut code, OUTPUT + 64);
        code.push(0xcc);
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(u64::from(CODE), &code).unwrap();
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        p.cpu.eip = CODE;
        p.cpu.set_register(Register32::Esp, STACK);
        let mut buttons = [false; 8];
        buttons[0] = true;
        buttons[7] = true;
        let before = p.cpu;
        p.submit_mouse_input(MouseInput {
            relative: [4, -6],
            wheel_steps: 2,
            buttons,
        })
        .unwrap();
        p.submit_mouse_input(MouseInput {
            relative: [-1, 2],
            wheel_steps: -1,
            buttons,
        })
        .unwrap();
        assert_eq!(p.cpu, before);
        let first = phase(&mut p, budget);
        let expected_buttons = [0x80, 0, 0, 0, 0, 0, 0, 0x80];
        assert_eq!(state(&p, OUTPUT), ([3, -4, 120], expected_buttons));
        assert_eq!(state(&p, OUTPUT + 32), ([0; 3], expected_buttons));
        p.submit_mouse_input(MouseInput {
            relative: [-3, 4],
            wheel_steps: -2,
            buttons: [false; 8],
        })
        .unwrap();
        let second = phase(&mut p, budget);
        assert_eq!(state(&p, OUTPUT + 64), ([-3, 4, -240], [0; 8]));
        assert_eq!(call(&mut p, API, &[DEVICE, 20, OUTPUT + 96]), 0);
        assert_eq!(state(&p, OUTPUT + 96), ([0; 3], [0; 8]));
        results.push((p.cpu, first, second, p.memory.mapped_pages()));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
