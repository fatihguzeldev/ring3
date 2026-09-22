use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;

pub const NEXT: u32 = 0x7000_02c4;
pub const OLD: u32 = 0x0040_1180;
pub const NEW: u32 = 0x0040_11c0;
pub const DATA: u64 = 0x0040_2300;

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let saved = p.cpu;
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, 0x1000_c000);
    let bytes: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(0x1000_c000, &bytes).unwrap();
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x0040_10f0);
    assert_eq!(
        p.cpu.register(Register32::Esp),
        0x1000_c004 + 4 * u32::try_from(args.len()).unwrap()
    );
    let value = p.cpu.register(Register32::Eax);
    p.cpu = saved;
    value
}

pub fn forwarder() -> Vec<u8> {
    let mut code = [0xff, 0x74, 0x24, 12].repeat(3);
    code.extend_from_slice(&[0x68, 0xef, 0xbe, 0xad, 0xde, 0xb8]);
    code.extend_from_slice(&NEXT.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc2, 12, 0]);
    code
}

pub fn process(veto: u32) -> Process32 {
    let mut bytes = window_creation_executable::guest();
    let mut old = Vec::new();
    for offset in [4_u8, 8, 12] {
        old.extend_from_slice(&[0x8b, 0x44, 0x24, offset, 0xa3]);
        old.extend_from_slice(&(0x0040_2300_u32 + u32::from(offset) - 4).to_le_bytes());
    }
    old.extend_from_slice(&[0x8b, 0, 0xc7, 0x40, 20, 160, 0, 0, 0]);
    old.extend_from_slice(&[0x6a, 77, 0xb8, 0, 0, 0, 0x70, 0xff, 0xd0, 0xb8]);
    old.extend_from_slice(&veto.to_le_bytes());
    old.extend_from_slice(&[0xc2, 12, 0]);
    assert!(old.len() <= 64);
    bytes[0x380..0x380 + old.len()].copy_from_slice(&old);
    let new = forwarder();
    bytes[0x3c0..0x3c0 + new.len()].copy_from_slice(&new);
    let mut p = Process32::load(&bytes, 32).unwrap();
    assert_eq!(call(&mut p, 0x7000_024c, &[5, OLD, 0, 1]), 0x7400_0004);
    call(&mut p, 0x7000_024c, &[u32::MAX, 0xdead_beef, 0, 1]);
    call(&mut p, 0x7000_024c, &[5, NEW, 0, 1]);
    p
}

pub fn run(p: &mut Process32, budget: u64) -> (Cpu32, (u64, u64)) {
    let mut counts = (0, 0);
    loop {
        let result = p.run(budget);
        counts.0 += result.instructions;
        counts.1 += result.api_calls;
        if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            return (p.cpu, counts);
        }
        assert!(counts.0 + counts.1 < 5000);
    }
}

pub fn verify() {
    for veto in [0, 0xdead_beef] {
        let mut expected = None;
        for budget in [1, 1000] {
            let mut p = process(veto);
            let result = run(&mut p, budget);
            if let Some(previous) = expected {
                assert_eq!(result, previous);
            }
            expected = Some(result);
            assert_eq!(p.last_error().unwrap(), 77);
            assert_eq!(
                p.cpu.register(Register32::Ebx),
                if veto == 0 { 0x7500_0004 } else { 0 }
            );
            let mut words = [0; 12];
            p.memory.read(DATA, &mut words).unwrap();
            assert_eq!(&words[..8], &[3, 0, 0, 0, 4, 0, 0, 0x75]);
            assert_ne!(&words[8..], &[0; 4]);
            if veto == 0 {
                assert_eq!(call(&mut p, 0x7000_02b0, &[0x7500_0004, 0x0040_2320]), 1);
                let mut rect = [0; 16];
                p.memory.read(DATA + 32, &mut rect).unwrap();
                assert_eq!(
                    rect,
                    [10_i32, 20, 170, 110]
                        .map(i32::to_le_bytes)
                        .concat()
                        .as_slice()
                );
            }
        }
    }
}
