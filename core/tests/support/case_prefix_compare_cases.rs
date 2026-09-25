use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

pub const LEFT: u32 = 0x0040_2180;
pub const RIGHT: u32 = 0x0040_21c0;

pub fn executable(left: &[u8], right: &[u8], count: u32) -> Vec<u8> {
    let mut code = Vec::new();
    for value in [count, RIGHT, LEFT] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 12, 0xcc]);
    let mut bytes = super::imported_executable::pe32(&code, "mSvCrT.dll", &["_strnicmp"]);
    bytes[0x580..0x580 + left.len()].copy_from_slice(left);
    bytes[0x5c0..0x5c0 + right.len()].copy_from_slice(right);
    bytes
}

pub fn imported_results_across_budgets() {
    for (left, right, count, expected) in [
        (b"MiXeD!".as_slice(), b"mixed?".as_slice(), 5, 0_i32),
        (b"MiXeD!", b"mixed?", 6, -30),
        (b"mixed?", b"MiXeD!", 6, 30),
        (b"\0unused", b"\0other", 7, 0),
        (b"a\0", b"AB", 2, -98),
        (b"AB", b"a\0", 2, 98),
        (b"Z", b"[", 1, 31),
        (b"_", b"A", 1, -2),
        (b"\xc4", b"\xe4", 1, -32),
        (b"\xff", b"\x7f", 1, 128),
        (b"different", b"content", 0, 0),
        (b"AZ", b"az", 2, 0),
        (b"abc\0D", b"ABC\0Z", 5, 0),
        (
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            b"abcdefghijklmnopqrstuvwxyz",
            26,
            0,
        ),
    ] {
        for budget in [1, 4, 30] {
            let mut p = Process32::load(&executable(left, right, count), 32).unwrap();
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 30);
            }
            assert_eq!(counts, (6, 1));
            assert_eq!(p.cpu.register(Register32::Eax), expected.cast_unsigned());
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        }
    }
}
