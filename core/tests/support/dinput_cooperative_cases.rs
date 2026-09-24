use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;

pub const DATA: u32 = 0x0040_2400;
pub const WINDOW: u32 = 0x7500_0004;
const ENTRY: u32 = 0x0040_1140;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

fn code() -> Vec<u8> {
    let mut code = Vec::new();
    for value in [0, DATA, 0x700, 0x0040_0000] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&0x0040_2290_u32.to_le_bytes());
    store(&mut code, DATA + 16);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    for value in [0, DATA + 4, DATA + 64] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 12]);
    store(&mut code, DATA + 20);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 24);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&(DATA + 4).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    for (flags, offset) in [(6, 28), (0x16, 32)] {
        push(&mut code, flags);
        push(&mut code, WINDOW);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 52]);
        store(&mut code, DATA + offset);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 36);
    code.push(0xcc);
    code
}

pub fn process() -> Process32 {
    let mut bytes = window_creation_executable::guest();
    bytes.resize(2048, 0);
    for (offset, value) in [
        (0x104, 60_u32),
        (0x1b0, 1024),
        (0x414, 0x2280),
        (0x420, 0x22c0),
        (0x424, 0x2290),
        (0x680, 0x22e0),
        (0x690, 0x22e0),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x6c0..0x6cb].copy_from_slice(b"dinput.dll\0");
    bytes[0x6e2..0x6f5].copy_from_slice(b"DirectInputCreateA\0");
    let code = code();
    assert!(code.len() <= 0xc0);
    bytes[0x340..0x340 + code.len()].copy_from_slice(&code);
    let mut p = Process32::load(&bytes, 64).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), WINDOW);
    p.memory
        .write(
            u64::from(DATA + 64),
            &[
                0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0, 0,
            ],
        )
        .unwrap();
    p.cpu.eip = ENTRY;
    p
}

pub fn imported_foreground_keyboard_setting_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = process();
        let windows = p.window_snapshots();
        let stack = p.cpu.register(Register32::Esp);
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            assert!(counts.0 < 1000);
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        let mut outputs = [0; 24];
        p.memory.read(u64::from(DATA + 16), &mut outputs).unwrap();
        assert_eq!(outputs, [0; 24]);
        assert_eq!(p.window_snapshots(), windows);
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 6);
        results.push((p.cpu, counts, outputs));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
