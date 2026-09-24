use ring3_core::execution::{
    Access, Cpu32, GuestModule, MemoryError, Permissions, Process32, ProcessOptions, ProcessStop,
    Register32, StopReason,
};

use super::{dll_executable, imported_executable};

pub const ENTER: u32 = 0x7000_0ff0;
pub const RETURN: u32 = 0x7000_0fec;
pub const CODE: u32 = 0x0040_1000;
pub const ENTRY: u32 = CODE + 0x40;
pub const LOG: u32 = 0x0040_2300;
pub const FIRST: u32 = 0x5000_0000;
pub const SECOND: u32 = 0x5001_0000;
pub const THIRD: u32 = 0x5002_0000;

pub fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn put(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

pub fn library(base: u32, digit: u8, dependency: Option<&str>) -> Vec<u8> {
    let mut code = vec![0x83, 0x7c, 0x24, 8, 2, 0x75, 0];
    for (stack, field) in [(4, 0x2190), (8, 0x2194), (12, 0x2198)] {
        code.extend_from_slice(&[0x8b, 0x44, 0x24, stack, 0xa3]);
        code.extend_from_slice(&(base + field).to_le_bytes());
    }
    code.extend_from_slice(&[0x64, 0xa1, 0x24, 0, 0, 0, 0xa3]);
    code.extend_from_slice(&(base + 0x218c).to_le_bytes());
    code.push(0xa1);
    code.extend_from_slice(&LOG.to_le_bytes());
    code.extend_from_slice(&[0xc1, 0xe0, 4, 0x83, 0xc8, digit, 0xa3]);
    code.extend_from_slice(&LOG.to_le_bytes());
    code.extend_from_slice(&[0x31, 0xc0, 0xc2, 12, 0]);
    code[6] = u8::try_from(code.len() - 7).unwrap();
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    dll_executable::dll(base, &code, dependency)
}

pub fn loaded(deferred_third: bool) -> Process32 {
    let mut code = vec![0xcc; 0x40];
    code.extend_from_slice(&[0x8b, 0x44, 0x24, 4, 0xa3]);
    code.extend_from_slice(&(LOG + 4).to_le_bytes());
    code.push(0xcc);
    let first = library(FIRST, 1, None);
    let second = library(SECOND, 2, Some("first.dll"));
    let third = library(THIRD, 3, None);
    let providers = [
        GuestModule {
            name: "second.dll",
            bytes: &second,
        },
        GuestModule {
            name: "first.dll",
            bytes: &first,
        },
        GuestModule {
            name: "third.dll",
            bytes: &third,
        },
    ];
    let split = if deferred_third { 2 } else { 3 };
    Process32::load_with_options(
        &imported_executable::pe32(&code, "MSVCRT.dll", &["_beginthreadex"]),
        128,
        ProcessOptions {
            modules: &providers[..split],
            deferred_modules: &providers[split..],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

pub fn process(deferred_third: bool) -> Process32 {
    let mut p = loaded(deferred_third);
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0);
    p
}

pub fn call(p: &mut Process32, offset: u32, args: &[u32]) -> u32 {
    let saved = p.cpu;
    prepare_at(p, offset, args, 0x1000_b000);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, CODE);
    let value = p.cpu.register(Register32::Eax);
    p.cpu = saved;
    value
}

pub fn prepare_at(p: &mut Process32, offset: u32, args: &[u32], stack: u32) {
    put(p, stack, &[CODE]);
    put(p, stack + 4, args);
    p.cpu.eip = 0x7000_0000 + offset;
    p.cpu.set_register(Register32::Esp, stack);
}

pub fn child(p: &mut Process32, argument: u32) -> (u32, Cpu32) {
    let handle = call(p, 0x548, &[0, 0, ENTRY, argument, 4, LOG + 8]);
    let id = word(p, LOG + 8);
    let teb = 0x1101_0000 + (id - 2) * 0x11000;
    let mut cpu = Cpu32::new(ENTER);
    cpu.set_fs_base(teb);
    cpu.set_register(Register32::Esp, teb - 8);
    cpu.set_x87_control_word(0x027f);
    (handle, cpu)
}

pub fn ordered_notifications_precede_each_child_entry() {
    let mut results = Vec::new();
    for budget in [1, 1000] {
        let mut p = process(false);
        assert_eq!(call(&mut p, 0xfc, &[THIRD]), 1);
        let mut counts = (0, 0);
        for argument in [42, 73] {
            let (handle, cpu) = child(&mut p, argument);
            assert_eq!(call(&mut p, 0x21c, &[handle]), 1);
            p.cpu = cpu;
            let mut complete = false;
            for _ in 0..1000 {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                    complete = true;
                    break;
                }
                assert_eq!(
                    run.reason,
                    ProcessStop::Stopped(StopReason::InstructionLimit)
                );
            }
            assert!(complete);
            assert_eq!(word(&p, LOG + 4), argument);
            assert_eq!(
                p.cpu.register(Register32::Esp),
                cpu.register(Register32::Esp)
            );
            for base in [FIRST, SECOND] {
                assert_eq!(word(&p, base + 0x2190), base);
                assert_eq!(word(&p, base + 0x2194), 2);
                assert_eq!(word(&p, base + 0x2198), 0);
                assert_eq!(word(&p, base + 0x218c), word(&p, LOG + 8));
            }
            assert_eq!(word(&p, THIRD + 0x2194), 0);
        }
        assert_eq!(word(&p, LOG), 0x1212);
        assert_eq!(counts.1, 0);
        results.push((p.cpu, counts));
    }
    assert_eq!(results[0], results[1]);
}

pub fn reach_return(p: &mut Process32) {
    for _ in 0..100 {
        if p.cpu.eip == RETURN {
            return;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    panic!("notification did not return");
}

pub fn retryable_notification_frames() {
    for first in [true, false] {
        let mut p = process(false);
        let (_, cpu) = child(&mut p, 42);
        p.cpu = cpu;
        if !first {
            reach_return(&mut p);
        }
        let before = p.cpu;
        let frame = cpu.register(Register32::Esp) - 16;
        let words = [0, 4, 8, 12].map(|offset| word(&p, frame + offset));
        let page = u64::from(frame & !0xfff);
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
        for _ in 0..2 {
            let run = p.run(1);
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
                    address: u64::from(frame),
                    access: Access::Write
                }))
            );
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!([0, 4, 8, 12].map(|offset| word(&p, frame + offset)), words);
        }
        p.memory
            .protect(page, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(
            p.run(1000).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(word(&p, LOG), 0x123);
        assert_eq!(word(&p, LOG + 4), 42);
    }
}
