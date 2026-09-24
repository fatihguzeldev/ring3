use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;
pub const UNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

pub fn imported_root_lifetime_across_budgets() {
    let mut code = Vec::new();
    for value in [0, DATA, 0x700, 0x0040_0000] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&0x0040_2060_u32.to_le_bytes());
    store(&mut code, DATA + 16);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b, 0x53, 0xff, 0x55, 4]);
    store(&mut code, DATA + 20);
    push(&mut code, DATA + 4);
    push(&mut code, DATA + 64);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 0]);
    store(&mut code, DATA + 24);
    for offset in [28, 32, 36] {
        code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
        store(&mut code, DATA + offset);
    }
    code.push(0xcc);
    let bytes = imported_executable::pe32(&code, "DINPUT.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        p.memory.write(u64::from(DATA + 64), &UNKNOWN).unwrap();
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
        let mut words = [0; 10];
        for (index, word) in words.iter_mut().enumerate() {
            let mut bytes = [0; 4];
            p.memory
                .read(u64::from(DATA) + index as u64 * 4, &mut bytes)
                .unwrap();
            *word = u32::from_le_bytes(bytes);
        }
        assert_ne!(words[0], 0);
        assert_eq!(words[0], words[1]);
        assert_eq!(&words[4..], &[0, 2, 0, 2, 1, 0]);
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 6);
        results.push((p.cpu, counts, words));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
