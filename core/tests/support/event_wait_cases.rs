use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;

fn immediate(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, offset: u32) {
    immediate(code, 0xa3, DATA + offset);
}

fn imported(code: &mut Vec<u8>, slot: u32) {
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&(0x0040_2060 + slot * 4).to_le_bytes());
}

fn direct(code: &mut Vec<u8>, offset: u32) {
    immediate(code, 0xb8, 0x7000_0000 + offset);
    code.extend_from_slice(&[0xff, 0xd0]);
}

fn handle(code: &mut Vec<u8>, offset: u32) {
    code.extend_from_slice(&[0xff, 0x35]);
    code.extend_from_slice(&(DATA + offset).to_le_bytes());
}

fn priority(code: &mut Vec<u8>, value: u32) {
    immediate(code, 0x68, value);
    immediate(code, 0x68, u32::MAX - 1);
    direct(code, 0x254);
}

pub fn executable(manual: bool) -> Vec<u8> {
    let mut code = Vec::new();
    for offset in [0, 4] {
        for value in [0, 0, u32::from(manual), 0] {
            immediate(&mut code, 0x68, value);
        }
        imported(&mut code, 0);
        store(&mut code, offset);
    }
    priority(&mut code, 2);
    for value in [0, 4, DATA, 0x0040_1100, 0, 0] {
        immediate(&mut code, 0x68, value);
    }
    direct(&mut code, 0x548);
    code.extend_from_slice(&[0x83, 0xc4, 24, 0x50]);
    imported(&mut code, 3);
    immediate(&mut code, 0x68, u32::MAX);
    handle(&mut code, 0);
    imported(&mut code, 2);
    store(&mut code, 16);
    handle(&mut code, 0);
    direct(&mut code, 0x21c);
    immediate(&mut code, 0xb8, 1);
    store(&mut code, 8);
    handle(&mut code, 4);
    imported(&mut code, 1);
    priority(&mut code, 0);
    code.extend_from_slice(&[0x64, 0xa1, 0x24, 0, 0, 0]);
    store(&mut code, 28);
    code.extend_from_slice(&[0x83, 0x3d]);
    code.extend_from_slice(&(DATA + 12).to_le_bytes());
    code.extend_from_slice(&[1, 0x75, 0xf7, 0xcc]);
    assert!(code.len() < 256);
    code.resize(256, 0xcc);
    code.extend_from_slice(&[0x8b, 0x5c, 0x24, 4, 0x89, 0xd8]);
    store(&mut code, 32);
    code.extend_from_slice(&[0x8b, 0x43, 8]);
    store(&mut code, 36);
    code.extend_from_slice(&[0xff, 0x33]);
    imported(&mut code, 1);
    immediate(&mut code, 0x68, u32::MAX);
    code.extend_from_slice(&[0xff, 0x73, 4]);
    imported(&mut code, 2);
    store(&mut code, 20);
    code.extend_from_slice(&[0x64, 0xa1, 0x24, 0, 0, 0]);
    store(&mut code, 24);
    code.extend_from_slice(&[0x8b, 0x43, 8]);
    store(&mut code, 12);
    code.extend_from_slice(&[0xeb, 0xfe]);
    imported_executable::pe32(
        &code,
        "kernel32.dll",
        &[
            "CreateEventA",
            "SetEvent",
            "WaitForSingleObject",
            "ResumeThread",
        ],
    )
}

pub fn event_handshake_runs_across_host_budgets() {
    for manual in [false, true] {
        let mut results = Vec::new();
        for budget in [1, 7, 4096, 20000] {
            let mut p = Process32::load(&executable(manual), 64).unwrap();
            let mut counts = (0, 0);
            let mut complete = false;
            for _ in 0..20000 {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                    complete = true;
                    break;
                }
                assert_eq!(
                    run.reason,
                    ProcessStop::Stopped(StopReason::InstructionLimit),
                    "cpu {:?}",
                    p.cpu
                );
            }
            assert!(complete);
            assert_eq!(counts.1, 11);
            let mut bytes = [0; 40];
            p.memory.read(u64::from(DATA), &mut bytes).unwrap();
            for (offset, expected) in [
                (8, 1),
                (12, 1),
                (16, 0),
                (20, 0),
                (24, 2),
                (28, 1),
                (32, DATA),
                (36, 0),
            ] {
                assert_eq!(
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()),
                    expected
                );
            }
            assert_eq!(p.cpu.fs_base(), 0x7ffd_e000);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            results.push((p.cpu, counts, bytes));
        }
        assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    }
}
