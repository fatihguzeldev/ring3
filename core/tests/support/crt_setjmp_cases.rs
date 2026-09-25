use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

pub const ENV: u32 = 0x0040_2180;
pub const REGISTRATION: u32 = 0x0040_21d0;
pub const OPTIONAL: [u32; 8] = [0x1234_5678, u32::MAX, 11, 22, 33, 44, 55, 66];
pub const SAVED: [(Register32, u32); 4] = [
    (Register32::Ebp, 0x1122_3344),
    (Register32::Ebx, 0x5566_7788),
    (Register32::Edi, 0x99aa_bbcc),
    (Register32::Esi, 0xddee_ff00),
];

pub fn executable(count: u32) -> (Vec<u8>, u32) {
    let mut code = Vec::new();
    for &word in OPTIONAL[..count as usize].iter().rev() {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    for word in [count, ENV] {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    let returned = 0x0040_1000 + u32::try_from(code.len()).unwrap();
    code.extend([0x83, 0xc4, u8::try_from((count + 2) * 4).unwrap(), 0xcc]);
    let mut bytes = super::imported_executable::pe32(&code, "MSVCRT.dll", &["_setjmp3"]);
    bytes[0x580..0x5c0].fill(0xa5);
    (bytes, returned)
}

pub fn imported_capture_across_budgets() {
    for count in [0, 1, 2, 3, 8] {
        for active in [false, true] {
            for budget in [1, 4, 80] {
                let (exe, returned) = executable(count);
                let mut p = Process32::load(&exe, 32).unwrap();
                for (register, value) in SAVED {
                    p.cpu.set_register(register, value);
                }
                if active {
                    p.memory
                        .write(0x7ffd_e000, &REGISTRATION.to_le_bytes())
                        .unwrap();
                    p.memory
                        .write(u64::from(REGISTRATION + 12), &7_u32.to_le_bytes())
                        .unwrap();
                }
                let mut counts = (0, 0);
                loop {
                    let run = p.run(budget);
                    counts.0 += run.instructions;
                    counts.1 += run.api_calls;
                    if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                        break;
                    }
                    assert!(counts.0 + counts.1 < 80);
                }
                assert_eq!(counts, (u64::from(count) + 5, 1));
                assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
                assert_eq!(p.cpu.register(Register32::Eax), 0);
                for (register, value) in SAVED {
                    assert_eq!(p.cpu.register(register), value);
                }
                let mut bytes = [0; 64];
                p.memory.read(u64::from(ENV), &mut bytes).unwrap();
                let actual: Vec<_> = bytes
                    .chunks_exact(4)
                    .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
                    .collect();
                let mut expected = [0xa5a5_a5a5; 16];
                expected[..10].copy_from_slice(&[
                    SAVED[0].1,
                    SAVED[1].1,
                    SAVED[2].1,
                    SAVED[3].1,
                    0x1001_0000 - (count + 3) * 4,
                    returned,
                    if active { REGISTRATION } else { u32::MAX },
                    if active && count < 2 { 7 } else { u32::MAX },
                    0x5643_3230,
                    if active && count != 0 { OPTIONAL[0] } else { 0 },
                ]);
                if active && count > 2 {
                    expected[10..count as usize + 8].copy_from_slice(&OPTIONAL[2..count as usize]);
                }
                assert_eq!(actual, expected);
            }
        }
    }
}
