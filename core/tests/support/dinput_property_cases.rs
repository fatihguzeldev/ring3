use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;
pub const HEADER: u32 = 0x3000_0001;
pub const KEYBOARD: [u8; 16] = [
    0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

pub fn imported_keyboard_buffer_setting_across_budgets() {
    imported_buffer_setting_across_budgets(KEYBOARD);
}

pub fn imported_buffer_setting_across_budgets(guid: [u8; 16]) {
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
    for index in 0..3 {
        push(&mut code, HEADER + index * 32);
        push(&mut code, 1);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 24]);
        store(&mut code, DATA + 28 + index * 4);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 40);
    code.push(0xcc);
    let bytes = imported_executable::pe32(&code, "dinput.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        p.memory
            .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        for (index, value) in (0..).zip([16, 1025, 0]) {
            let header: Vec<_> = [20_u32, 16, 0, 0, value]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            p.memory
                .write(u64::from(HEADER + index * 32), &header)
                .unwrap();
        }
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ)
            .unwrap();
        p.memory.write(u64::from(DATA + 64), &guid).unwrap();
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
        let mut outputs = [0; 28];
        p.memory.read(u64::from(DATA + 16), &mut outputs).unwrap();
        assert_eq!(outputs, [0; 28]);
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(counts.1, 7);
        results.push((p.cpu, counts, outputs));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
