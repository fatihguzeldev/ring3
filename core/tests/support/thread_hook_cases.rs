use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const DATA: u32 = 0x0040_2180;

fn word(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    code.extend_from_slice(&value.to_le_bytes());
}

fn api(code: &mut Vec<u8>, offset: u32) {
    word(code, 0xb8, 0x7000_0000 + offset);
    code.extend_from_slice(&[0xff, 0xd0]);
}

fn stored(code: &mut Vec<u8>, offset: u32) {
    code.extend_from_slice(&[0xff, 0x35]);
    code.extend_from_slice(&(DATA + offset).to_le_bytes());
}

pub fn executable() -> Vec<u8> {
    with_hooks([(u32::MAX, 0), (5, 0)])
}

fn with_hooks(hooks: [(u32, u32); 2]) -> Vec<u8> {
    let mut code = Vec::new();
    for value in [0, 0, 1, 0] {
        word(&mut code, 0x68, value);
    }
    api(&mut code, 0x53c);
    word(&mut code, 0xa3, DATA);
    for value in [0, 4, 0, 0x0040_1100, 0, 0] {
        word(&mut code, 0x68, value);
    }
    api(&mut code, 0x548);
    code.extend_from_slice(&[0x83, 0xc4, 24, 0x50]);
    api(&mut code, 0x550);
    word(&mut code, 0x68, u32::MAX);
    stored(&mut code, 0);
    api(&mut code, 0x214);
    for offset in [4, 8] {
        stored(&mut code, offset);
        code.extend_from_slice(&[0xff, 0x15, 0x64, 0x20, 0x40, 0]);
        word(&mut code, 0xa3, DATA + offset + 8);
    }
    code.push(0xcc);
    assert!(code.len() < 256);
    code.resize(256, 0xcc);
    api(&mut code, 0xdc);
    code.extend_from_slice(&[0x89, 0xc3]);
    for ((kind, module), offset) in hooks.into_iter().zip([4, 8]) {
        code.push(0x53);
        word(&mut code, 0x68, module);
        word(&mut code, 0x68, 0xdead_beef);
        word(&mut code, 0x68, kind);
        code.extend_from_slice(&[0xff, 0x15, 0x60, 0x20, 0x40, 0]);
        word(&mut code, 0xa3, DATA + offset);
    }
    stored(&mut code, 0);
    api(&mut code, 0x540);
    code.extend_from_slice(&[0xeb, 0xfe]);
    imported_executable::pe32(
        &code,
        "user32.dll",
        &["SetWindowsHookExA", "UnhookWindowsHookEx"],
    )
}

pub fn scheduled_hook_lifetimes() {
    verify_lifetimes(&executable());
}

pub fn scheduled_keyboard_hook_lifetimes() {
    verify_lifetimes(&with_hooks([(2, 0x0040_0000), (2, 0)]));
}

fn verify_lifetimes(executable: &[u8]) {
    let mut expected = None;
    for budget in [1, 7, 4096, 20000] {
        let mut p = Process32::load(executable, 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert!(counts.0 + counts.1 < 20000);
        }
        let mut bytes = [0; 16];
        p.memory.read(u64::from(DATA + 4), &mut bytes).unwrap();
        assert_eq!(
            bytes,
            [0x7400_0004_u32, 0x7400_0008, 1, 1]
                .map(u32::to_le_bytes)
                .concat()
                .as_slice()
        );
        assert_eq!(p.cpu.fs_base(), 0x7ffd_e000);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(counts.1, 10);
        let result = (p.cpu, counts, bytes);
        if let Some(previous) = expected {
            assert_eq!(result, previous);
        }
        expected = Some(result);
    }
}
