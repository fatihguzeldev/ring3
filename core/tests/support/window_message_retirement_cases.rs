use ring3_core::execution::{PostedMessage, Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;
use super::window_message_cases::{STACK, WINDOW, call, prepare};

pub const DIALOG: u32 = WINDOW + 4;
pub const CHILD: u32 = DIALOG + 4;
pub const PROCEDURE: u32 = 0x0040_1180;
pub const OUTPUT: u32 = 0x1000_c100;
pub const DESTROY: u32 = 0x7000_0470;

pub fn created() -> Process32 {
    let mut bytes = window_creation_executable::guest();
    bytes[0x380..0x388].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 16, 0]);
    let mut p = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let mut template = Vec::new();
    for value in [
        1_u16, 0xffff, 0, 0, 0, 0, 0, 0x1000, 1, 0, 0, 40, 30, 0, 0, 68, 0, 0,
    ] {
        template.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 0, 0x4001_0000, 0, 0x000a_0014, 1, 0x0080_ffff, 0] {
        template.extend_from_slice(&value.to_le_bytes());
    }
    p.memory.write(0x1000_c000, &template).unwrap();
    prepare(
        &mut p,
        0x7000_0434,
        &[0x0040_0000, 0x1000_c000, 0, PROCEDURE, 0],
    );
    finish(&mut p, 100);
    assert_eq!(p.cpu.register(Register32::Eax), DIALOG);
    assert_eq!(p.window_snapshots().len(), 3);
    p
}

pub fn finish(p: &mut Process32, budget: u64) -> (u64, u64) {
    let mut counts = (0, 0);
    loop {
        let result = p.run(budget);
        counts.0 += result.instructions;
        counts.1 += result.api_calls;
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            return counts;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert!(counts.0 + counts.1 < 1000);
    }
}

pub fn message(hwnd: u32, id: u32) -> PostedMessage {
    PostedMessage {
        hwnd,
        message: id,
        wparam: 19,
        lparam: 23,
        time: 77,
        point: [-2, 3],
    }
}

pub fn peek(p: &mut Process32, remove: bool) -> Option<[u32; 8]> {
    let result = call(p, 0x7000_043c, &[OUTPUT, 0, 0, 0, u32::from(remove)]);
    if result == 0 {
        return None;
    }
    let mut bytes = [0; 32];
    p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    Some(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

pub fn verify() {
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = created();
        p.post_message(message(DIALOG, 0)).unwrap();
        p.post_message(message(WINDOW, 0x401)).unwrap();
        assert_eq!(call(&mut p, 0x7000_0464, &[CHILD, 0x402, 5, 6]), 1);
        p.post_message(message(0, 0x403)).unwrap();
        p.post_message(message(DIALOG, 0x404)).unwrap();
        let pages = p.memory.mapped_pages();
        let error = p.last_error().unwrap();
        prepare(&mut p, DESTROY, &[DIALOG]);
        let counts = finish(&mut p, budget);
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Esp), STACK + 8);
        assert_eq!(p.window_snapshots().len(), 1);
        assert_eq!(p.window_snapshots()[0].hwnd, WINDOW);
        assert_eq!(p.last_error().unwrap(), error);
        for (hwnd, id) in [(WINDOW, 0x401), (0, 0x403)] {
            let expected = [hwnd, id, 19, 23, 77, (-2_i32).cast_unsigned(), 3, 0];
            assert_eq!(peek(&mut p, false), Some(expected));
            assert_eq!(peek(&mut p, true), Some(expected));
        }
        assert_eq!(peek(&mut p, true), None);
        assert_eq!(p.memory.mapped_pages(), pages);
        results.push((p.cpu, counts, p.window_snapshots()));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
