use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;
pub const KEYBOARD: [u8; 16] = [
    0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];
pub const DEVICE: [u8; 16] = [
    0x80, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

pub fn imported_keyboard_lifetime_across_budgets() {
    let mut code = Vec::new();
    for value in [0, DATA, 0x700, 0x0040_0000] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&0x0040_2060_u32.to_le_bytes());
    store(&mut code, DATA + 16);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    for value in [0, DATA + 5, DATA + 64] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 12]);
    store(&mut code, DATA + 20);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 24);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&(DATA + 5).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b, 0x53, 0xff, 0x55, 4]);
    store(&mut code, DATA + 28);
    push(&mut code, DATA + 12);
    push(&mut code, DATA + 80);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 0]);
    store(&mut code, DATA + 32);
    for offset in [36, 40, 44] {
        code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
        store(&mut code, DATA + offset);
    }
    code.push(0xcc);
    let bytes = imported_executable::pe32(&code, "dinput.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        p.memory.write(u64::from(DATA + 64), &KEYBOARD).unwrap();
        p.memory.write(u64::from(DATA + 80), &DEVICE).unwrap();
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
        let mut outputs = Vec::new();
        for offset in [0, 5, 12, 16, 20, 24, 28, 32, 36, 40, 44] {
            let mut bytes = [0; 4];
            p.memory.read(u64::from(DATA + offset), &mut bytes).unwrap();
            outputs.push(u32::from_le_bytes(bytes));
        }
        assert_eq!(&outputs[..3], &[0x7001_7800, 0x7001_7900, 0x7001_7900]);
        assert_eq!(&outputs[3..], &[0, 0, 0, 2, 0, 2, 1, 0]);
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 8);
        results.push((p.cpu, counts, outputs));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
