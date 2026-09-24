use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

pub const CODE: u32 = 0x0040_1000;
pub const DATA: u32 = 0x0040_2180;
pub const PRIMARY: u32 = 0x7ffd_e000;
pub const CHILD: u32 = 0x1101_0000;
pub const CURRENT: u32 = u32::MAX - 1;

pub fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

pub fn prepare(p: &mut Process32, offset: u32, args: &[u32]) -> Cpu32 {
    let stack = if p.cpu.fs_base() == PRIMARY {
        0x1000_b000
    } else {
        p.cpu.fs_base() - 0x100
    };
    put(p, stack, CODE);
    for (i, &value) in args.iter().enumerate() {
        put(p, stack + 4 + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.eip = 0x7000_0000 + offset;
    p.cpu.set_register(Register32::Esp, stack);
    p.cpu
}

pub fn call(p: &mut Process32, offset: u32, args: &[u32]) -> u32 {
    let before = prepare(p, offset, args);
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.fs_base(), before.fs_base());
    assert_eq!(p.cpu.eip, CODE);
    p.cpu.register(Register32::Eax)
}

pub fn process() -> Process32 {
    let mut code = vec![0xcc; 16];
    code.extend_from_slice(&[
        0x8b, 0x44, 0x24, 4, 0x6a, 0xff, 0x50, 0xb8, 0x14, 2, 0, 0x70, 0xff, 0xd0, 0xcc,
    ]);
    code.resize(256, 0xcc);
    Process32::load(
        &super::imported_executable::pe32(&code, "kernel32.dll", &["WaitForSingleObject"]),
        96,
    )
    .unwrap()
}

pub fn park_child(p: &mut Process32, event: u32) -> u32 {
    let handle = call(p, 0x548, &[0, 0, CODE + 16, event, 4, 0]);
    assert_eq!(call(p, 0x254, &[handle, 1]), 1);
    assert_eq!(call(p, 0x550, &[handle]), 1);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(p.cpu.fs_base(), PRIMARY);
    assert_eq!(run.api_calls, 1);
    handle
}

pub fn denied(p: &mut Process32, offset: u32, args: &[u32]) {
    let before = prepare(p, offset, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::UnsupportedApi {
            address: before.eip
        }
    );
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}
