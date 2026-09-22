use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;

pub const PARENT: u32 = 0x7000_02b8;
pub const GET: u32 = 0x7000_02bc;
pub const SET: u32 = 0x7000_02c0;
pub const WINDOW: u32 = 0x7500_0004;
pub const INDEX: u32 = (-4_i32).cast_unsigned();
pub const REPLACEMENT: u32 = 0x0040_1180;
pub const STACK: u32 = 0x1000_ef00;

pub fn created() -> Process32 {
    let mut bytes = window_creation_executable::guest();
    bytes[0x380..0x38b].copy_from_slice(&[0x8b, 0x44, 0x24, 12, 3, 0x44, 0x24, 16, 0xc2, 16, 0]);
    let mut p = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        p.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), WINDOW);
    p
}

pub fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let bytes: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &bytes).unwrap();
    p.cpu
}

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_10f0;
    expected.set_register(Register32::Eax, result);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, expected);
    result
}

pub fn ownership() {
    let mut p = created();
    call(&mut p, 0x7000_0000, &[77]);
    assert_eq!(call(&mut p, PARENT, &[WINDOW]), 0);
    assert_eq!(call(&mut p, PARENT, &[1]), 0);
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
    assert_eq!(
        call(&mut p, SET, &[WINDOW, INDEX, REPLACEMENT]),
        0x0040_1100
    );
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), REPLACEMENT);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_ne!(
        call(
            &mut p,
            0x7000_0260,
            &[0x0040_0000, 0x0040_2180, 0x0040_2300]
        ),
        0
    );
    let mut procedure = [0; 4];
    p.memory.read(0x0040_2304, &mut procedure).unwrap();
    assert_eq!(u32::from_le_bytes(procedure), 0x0040_1100);
    prepare(&mut p, 0x7000_02a4, &[REPLACEMENT, WINDOW, 0x400, 10, 32]);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 42);
    assert_eq!(
        call(&mut p, SET, &[WINDOW, INDEX, 0x0040_1100]),
        REPLACEMENT
    );
    let mut other = created();
    assert_eq!(call(&mut other, GET, &[WINDOW, INDEX]), 0x0040_1100);
}
