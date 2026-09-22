use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;

pub const SEND: u32 = 0x7000_02cc;
pub const WINDOW: u32 = 0x7500_0004;
pub const PROCEDURE: u32 = 0x0040_1180;
pub const STACK: u32 = 0x1000_ef00;

pub fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|w| w.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
    p.cpu
}

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    expected.eip = 0x0040_10f0;
    assert_eq!(p.cpu, expected);
    value
}

pub fn created() -> Process32 {
    let mut bytes = window_creation_executable::guest();
    let mut code = Vec::new();
    for offset in [4_u8, 8, 12, 16] {
        code.extend_from_slice(&[0x8b, 0x44, 0x24, offset, 0xa3]);
        code.extend_from_slice(&(0x0040_2300_u32 + u32::from(offset) - 4).to_le_bytes());
    }
    code.extend_from_slice(&[
        0xff, 5, 0x10, 0x23, 0x40, 0, 0x6a, 77, 0xb8, 0, 0, 0, 0x70, 0xff, 0xd0, 0x8b, 0x44, 0x24,
        12, 3, 0x44, 0x24, 16, 0xc2, 16, 0,
    ]);
    assert!(code.len() <= 64);
    bytes[0x380..0x380 + code.len()].copy_from_slice(&code);
    let mut p = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        p.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), WINDOW);
    assert_eq!(
        call(
            &mut p,
            0x7000_02c0,
            &[WINDOW, (-4_i32).cast_unsigned(), PROCEDURE]
        ),
        0x0040_1100
    );
    p
}

pub fn verify() {
    let mut expected = None;
    for budget in [1, 1000] {
        let mut p = created();
        prepare(&mut p, SEND, &[WINDOW, 0x400, 10, 32]);
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 1000);
        }
        assert_eq!(p.cpu.register(Register32::Eax), 42);
        assert_eq!(p.cpu.register(Register32::Esp), STACK + 20);
        assert_eq!(p.last_error().unwrap(), 77);
        let mut data = [0; 20];
        p.memory.read(0x0040_2300, &mut data).unwrap();
        assert_eq!(
            data.as_slice(),
            [WINDOW, 0x400, 10, 32, 1].map(u32::to_le_bytes).concat()
        );
        if let Some(prior) = expected {
            assert_eq!((p.cpu, counts), prior);
        }
        expected = Some((p.cpu, counts));
    }
}
