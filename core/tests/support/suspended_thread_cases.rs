use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

const DATA: u32 = 0x0040_2180;

fn immediate(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, offset: u32) {
    immediate(code, 0xa3, DATA + offset);
}

fn call(code: &mut Vec<u8>, slot: u32, offset: u32) {
    code.extend_from_slice(&[0x53, 0xff, 0x15]);
    code.extend_from_slice(&(0x0040_2060 + slot * 4).to_le_bytes());
    store(code, offset);
}

fn counter(code: &mut Vec<u8>, offset: u32) {
    immediate(code, 0xa1, DATA);
    store(code, offset);
}

pub fn suspended_guest_preserves_counts_across_host_budgets() {
    let mut code = Vec::new();
    for value in [0, 4, 0, 0x0040_1100, 0, 0] {
        immediate(&mut code, 0x68, value);
    }
    immediate(&mut code, 0xb8, 0x7000_0548);
    code.extend_from_slice(&[0xff, 0xd0, 0x83, 0xc4, 24, 0x89, 0xc3]);
    call(&mut code, 0, 4);
    call(&mut code, 1, 8);
    counter(&mut code, 12);
    call(&mut code, 1, 16);
    call(&mut code, 1, 20);
    call(&mut code, 0, 24);
    counter(&mut code, 28);
    immediate(&mut code, 0xb9, 5000);
    code.extend_from_slice(&[0x49, 0x75, 0xfd]);
    counter(&mut code, 32);
    call(&mut code, 1, 36);
    counter(&mut code, 40);
    code.push(0xcc);
    assert!(code.len() < 256);
    code.resize(256, 0xcc);
    code.extend_from_slice(&[0xff, 0x05]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0xeb, 0xf8]);
    let executable =
        imported_executable::pe32(&code, "kernel32.dll", &["SuspendThread", "ResumeThread"]);
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(&executable, 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            assert!(counts.0 < 50000);
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit),
                "budget {budget}, cpu {:?}",
                p.cpu
            );
        }
        assert_eq!(counts.1, 7);
        let mut bytes = [0; 44];
        p.memory.read(u64::from(DATA), &mut bytes).unwrap();
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        for (offset, expected) in [(4, 1), (8, 2), (12, 0), (16, 1), (20, 0), (24, 0), (36, 1)] {
            assert_eq!(word(offset), expected);
        }
        assert!(word(28) > 1000);
        assert_eq!(word(28), word(32));
        assert!(word(40) > word(32) + 1000);
        assert_eq!(p.cpu.fs_base(), 0x7ffd_e000);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        results.push((p.cpu, counts, bytes));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
