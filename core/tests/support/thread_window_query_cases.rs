use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

use super::window_creation_executable;

pub const DATA: u32 = 0x0040_2400;
pub const CHILD: u32 = 0x1101_0000;
pub const HANDLE: u32 = 0x7500_0004;
pub const ENTRY: u32 = 0x0040_1400;

pub fn queries() -> Vec<(u32, Vec<u32>)> {
    vec![
        (0x2b8, vec![HANDLE]),
        (0x2b0, vec![HANDLE, DATA + 128]),
        (0x2b4, vec![HANDLE, DATA + 144]),
        (0x2bc, vec![HANDLE, (-4_i32).cast_unsigned()]),
        (0x2bc, vec![HANDLE, (-16_i32).cast_unsigned()]),
        (0x2bc, vec![HANDLE, (-20_i32).cast_unsigned()]),
        (0x10, vec![]),
        (0x26c, vec![0x0040_2180, 0x0040_2190]),
        (0x270, vec![HANDLE]),
        (0x438, vec![HANDLE, 42]),
        (0x454, vec![0]),
        (0x458, vec![HANDLE, 0]),
    ]
}

pub fn put(p: &mut Process32, address: u32, words: &[u32]) {
    p.memory
        .write(
            u64::from(address),
            &words
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}

pub fn prepare(p: &mut Process32, offset: u32, args: &[u32]) -> Cpu32 {
    let stack = if p.cpu.fs_base() == CHILD {
        CHILD - 128
    } else {
        0x1000_b000
    };
    put(p, stack, &[ENTRY]);
    put(p, stack + 4, args);
    p.cpu.eip = 0x7000_0000 + offset;
    p.cpu.set_register(Register32::Esp, stack);
    p.cpu
}

pub fn call(p: &mut Process32, offset: u32, args: &[u32]) -> u32 {
    let saved = p.cpu;
    let mut expected = prepare(p, offset, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = ENTRY;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        expected.register(Register32::Esp)
            + if offset == 0x548 {
                4
            } else {
                4 * u32::try_from(args.len() + 1).unwrap()
            },
    );
    assert_eq!(p.cpu, expected);
    p.cpu = saved;
    value
}

fn immediate(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    code.extend_from_slice(&value.to_le_bytes());
}

pub fn process() -> Process32 {
    let mut p = Process32::load(&window_creation_executable::guest(), 64).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), HANDLE);
    let mut code = Vec::new();
    for (index, (offset, args)) in queries().into_iter().enumerate() {
        for value in args.into_iter().rev() {
            immediate(&mut code, 0x68, value);
        }
        immediate(&mut code, 0xb8, 0x7000_0000 + offset);
        code.extend_from_slice(&[0xff, 0xd0]);
        immediate(&mut code, 0xa3, DATA + 4 * u32::try_from(index).unwrap());
    }
    code.push(0xcc);
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(ENTRY), &code).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    p
}

pub fn resume(p: &mut Process32) {
    let handle = call(p, 0x548, &[0, 0, ENTRY, 0, 4, 0]);
    assert_ne!(handle, 0);
    assert_eq!(call(p, 0x550, &[handle]), 1);
}

pub fn scheduled() -> Process32 {
    let mut p = process();
    resume(&mut p);
    for _ in 0..20 {
        if p.cpu.eip == 0x7000_02b8 {
            assert_eq!(p.cpu.fs_base(), CHILD);
            return p;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    panic!("child query not reached");
}

pub fn verify() {
    let mut expected = None;
    for budget in [1, 7, 4096, 20000] {
        let mut p = process();
        let values: Vec<_> = queries()
            .into_iter()
            .map(|(offset, args)| call(&mut p, offset, &args))
            .collect();
        assert_eq!(
            values,
            [
                0,
                1,
                1,
                0x0040_1100,
                0x04ca_0000,
                0x100,
                1,
                HANDLE,
                1,
                0,
                HANDLE,
                HANDLE
            ]
        );
        let mut rectangles = [0; 32];
        p.memory
            .read(u64::from(DATA + 128), &mut rectangles)
            .unwrap();
        p.memory.write(u64::from(DATA), &[0; 160]).unwrap();
        let windows = p.window_snapshots();
        resume(&mut p);
        let pages = p.memory.mapped_pages();
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
            assert!(counts.0 + counts.1 < 1000);
        }
        assert_eq!(counts.1, 12);
        let mut bytes = [0; 160];
        p.memory.read(u64::from(DATA), &mut bytes).unwrap();
        assert_eq!(
            &bytes[..48],
            values
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>()
                .as_slice()
        );
        assert_eq!(&bytes[128..], &rectangles);
        assert_eq!(p.window_snapshots(), windows);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(p.cpu.fs_base(), CHILD);
        let result = (p.cpu, counts, bytes);
        if let Some(previous) = expected {
            assert_eq!(result, previous);
        }
        expected = Some(result);
    }
}
