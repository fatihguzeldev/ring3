use ring3_core::execution::{
    GuestMemory, Permissions, Process32, ProcessStop, Register32, StopReason,
};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;
pub const FORMAT: u32 = 0x3000_0001;
pub const KEY: u32 = 0x3000_0041;
pub const OBJECTS: u32 = 0x3000_1001;
pub const KEY_GUID: [u8; 16] = [
    0x20, 0x82, 0x72, 0x55, 0x3c, 0xd3, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];

pub fn standard(memory: &mut GuestMemory, reverse: bool) {
    let header: Vec<_> = [24_u32, 16, 2, 256, 256, OBJECTS]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    memory.write(u64::from(FORMAT), &header).unwrap();
    memory.write(u64::from(KEY), &KEY_GUID).unwrap();
    for entry in 0..256 {
        let key = if reverse { 255 - entry } else { entry };
        let object: Vec<_> = [KEY, key, 0x8000_000c | key << 8, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        memory
            .write(u64::from(OBJECTS + entry * 16), &object)
            .unwrap();
    }
}

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

pub fn imported_standard_keyboard_format_across_budgets() {
    imported_format_across_budgets(
        [
            0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0,
            0,
        ],
        standard,
    );
}

pub fn imported_format_across_budgets(guid: [u8; 16], setup: fn(&mut GuestMemory, bool)) {
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
    for offset in [28, 32] {
        push(&mut code, FORMAT);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 44]);
        store(&mut code, DATA + offset);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 36);
    code.push(0xcc);
    let bytes = imported_executable::pe32(&code, "dinput.dll", &["DirectInputCreateA"]);
    let mut results = Vec::new();
    for variant in [false, true] {
        for budget in [1, 7, 4096, 20000] {
            let mut p = Process32::load(&bytes, 64).unwrap();
            p.memory
                .map_zeroed(0x3000_0000, 12288, Permissions::READ_WRITE)
                .unwrap();
            setup(&mut p.memory, variant);
            p.memory
                .protect(0x3000_0000, 12288, Permissions::READ)
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
            let mut outputs = [0; 24];
            p.memory.read(u64::from(DATA + 16), &mut outputs).unwrap();
            assert_eq!(outputs, [0; 24]);
            assert_eq!(p.cpu.register(Register32::Esp), stack);
            assert_eq!(counts.1, 6);
            results.push((p.cpu, counts, outputs));
        }
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
