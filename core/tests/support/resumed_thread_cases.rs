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

fn imported_call(code: &mut Vec<u8>, slot: u32) {
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&(0x0040_2060 + slot * 4).to_le_bytes());
}

pub fn executable() -> Vec<u8> {
    let mut code = Vec::new();
    imported_call(&mut code, 1);
    immediate(&mut code, 0x68, 1234);
    code.push(0x50);
    imported_call(&mut code, 2);
    for value in [0, 4, 42, 0x0040_1100, 0, 0] {
        immediate(&mut code, 0x68, value);
    }
    immediate(&mut code, 0xb8, 0x7000_0548);
    code.extend_from_slice(&[0xff, 0xd0, 0x83, 0xc4, 24, 0x89, 0xc3]);
    store(&mut code, 60);
    code.extend_from_slice(&[0x68, 0, 0, 0x80, 0x3f, 0xd9, 0x04, 0x24, 0x83, 0xc4, 4]);
    immediate(&mut code, 0xb9, 0xcafe_1234);
    immediate(&mut code, 0x68, 0x46);
    code.extend_from_slice(&[0x9d, 0x50]);
    imported_call(&mut code, 0);
    store(&mut code, 0);
    for (modrm, offset) in [(0x1d, 20), (0x0d, 24)] {
        code.extend_from_slice(&[0x89, modrm]);
        code.extend_from_slice(&(DATA + offset).to_le_bytes());
    }
    code.extend_from_slice(&[0x9c, 0x58]);
    store(&mut code, 16);
    code.extend_from_slice(&[0xdd, 0x1d]);
    code.extend_from_slice(&(DATA + 32).to_le_bytes());
    code.extend_from_slice(&[0x64, 0xa1, 0x24, 0, 0, 0]);
    store(&mut code, 40);
    code.extend_from_slice(&[0x6a, 0]);
    imported_call(&mut code, 3);
    store(&mut code, 4);
    code.extend_from_slice(&[0x83, 0x3d]);
    code.extend_from_slice(&(DATA + 8).to_le_bytes());
    code.extend_from_slice(&[42, 0x75, 0xf7, 0xcc]);
    assert!(code.len() < 256);
    code.resize(256, 0xcc);
    code.extend_from_slice(&[0x8b, 0x44, 0x24, 4]);
    store(&mut code, 8);
    immediate(&mut code, 0xb9, 0xbad0_0002);
    immediate(&mut code, 0xbb, 0xbad0_0003);
    code.extend_from_slice(&[
        0x68, 0, 0, 0x80, 0x3f, 0xd9, 0x04, 0x24, 0x83, 0xc4, 4, 0xd9, 0xe0,
    ]);
    code.extend_from_slice(&[0x64, 0xa1, 0x24, 0, 0, 0]);
    store(&mut code, 44);
    code.extend_from_slice(&[0x6a, 0]);
    imported_call(&mut code, 3);
    store(&mut code, 48);
    immediate(&mut code, 0x68, 5678);
    code.extend_from_slice(&[0x6a, 0]);
    imported_call(&mut code, 2);
    code.extend_from_slice(&[0xff, 0x05]);
    code.extend_from_slice(&(DATA + 52).to_le_bytes());
    code.extend_from_slice(&[0xeb, 0xf8]);
    imported_executable::pe32(
        &code,
        "kernel32.dll",
        &["ResumeThread", "TlsAlloc", "TlsSetValue", "TlsGetValue"],
    )
}

pub fn resumed_guest_preserves_context_across_host_budgets() {
    let mut results = Vec::new();
    for budget in [1, 7, 4095, 4096, 20000] {
        let mut p = Process32::load(&executable(), 64).unwrap();
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
                "budget {budget}, cpu {:?}",
                p.cpu
            );
        }
        assert!(complete);
        assert_eq!(counts.1, 7);
        let mut bytes = [0; 64];
        p.memory.read(u64::from(DATA), &mut bytes).unwrap();
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        for (offset, expected) in [
            (0, 1),
            (4, 1234),
            (8, 42),
            (16, 0x46),
            (24, 0xcafe_1234),
            (40, 1),
            (44, 2),
            (48, 0),
        ] {
            assert_eq!(word(offset), expected);
        }
        assert_eq!(word(20), word(60));
        assert_ne!(word(60), 0);
        assert!(word(52) > 1000);
        assert_eq!(&bytes[32..40], &1.0_f64.to_le_bytes());
        assert_eq!(p.cpu.fs_base(), 0x7ffd_e000);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        results.push((p.cpu, counts, bytes));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
