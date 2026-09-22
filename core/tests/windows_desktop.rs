#[path = "support/desktop_query_executable.rs"]
mod desktop_query_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const DESKTOP: u32 = 0x7000_0010;
const FIND: u32 = 0x7000_026c;
const IS_WINDOW: u32 = 0x7000_0270;
const NAME: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn load() -> Process32 {
    Process32::load(&desktop_query_executable::pe32(), 32).unwrap()
}
fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Esp, STACK);
    for (i, word) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    p.cpu
}
fn call(p: &mut Process32, api: u32, args: &[u32], expected: u32) {
    let mut cpu = prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, expected);
    cpu.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, cpu);
}

#[test]
fn imported_desktop_queries_run_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = load();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (8, 3));
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Ebx), 1);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn desktop_exists_but_registered_names_and_foreign_handles_are_not_windows() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    call(&mut p, DESKTOP, &[], 1);
    call(&mut p, IS_WINDOW, &[1], 1);
    call(&mut p, 0x7000_0094, &[NAME], 0xc000);
    for (i, word) in [0_u32, 0x0040_1040, 0, 0, 0x0040_0000, 0, 0, 0, 0, NAME]
        .iter()
        .enumerate()
    {
        p.memory
            .write(0x0040_2200 + i as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    call(&mut p, 0x7000_0264, &[0x0040_2200], 0xc000);
    for handle in [0, 2, 0xc000, 0x0040_0000, 0x7000_1000, 0xffff_ffff] {
        call(&mut p, IS_WINDOW, &[handle], 0);
    }
    for class in [0, NAME, 0xc000, 0xffff] {
        for title in [0, NAME, NAME + 4] {
            call(&mut p, FIND, &[class, title], 0);
        }
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    call(&mut load(), FIND, &[NAME, 0], 0);
}

#[test]
fn query_strings_are_bounded_and_can_end_at_the_address_space_limit() {
    let mut p = load();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_f000, &[b'a'; 4095]).unwrap();
    call(&mut p, FIND, &[NAME, 0xffff_f000], 0);
    call(&mut p, FIND, &[0xffff_ff00, 0xffff_ffff], 0);
    p.memory
        .protect(0xffff_f000, 4096, Permissions::READ)
        .unwrap();
    call(&mut p, FIND, &[0xffff_fffe, 0xffff_fffe], 0);
    p.memory
        .protect(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_ffff, b"a").unwrap();
    for args in [[0xffff_ffff, 0], [0, 0xffff_ffff], [0, 1], [0x0040_3000, 0]] {
        let cpu = prepare(&mut p, FIND, &args);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, cpu);
    }
    for text in [b"\0".as_slice(), b"#32769\0", b"\x80\0", &[b'x'; 256]] {
        p.memory.write(u64::from(NAME), text).unwrap();
        let cpu = prepare(&mut p, FIND, &[NAME, 0]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: FIND }
        );
        assert_eq!(p.cpu, cpu);
    }
    for args in [[1, 0], [0xbfff, 0], [0, 0xffff_f000]] {
        let cpu = prepare(&mut p, FIND, &args);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: FIND }
        );
        assert_eq!(p.cpu, cpu);
    }
    p.memory.write(u64::from(NAME), b"\x80\0").unwrap();
    let cpu = prepare(&mut p, FIND, &[0, NAME]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: FIND }
    );
    assert_eq!(p.cpu, cpu);
}

#[test]
fn zero_budgets_and_faulting_frames_leave_queries_untouched() {
    let mut p = load();
    for (api, args) in [
        (DESKTOP, &[][..]),
        (IS_WINDOW, &[1][..]),
        (FIND, &[NAME, 0][..]),
    ] {
        let cpu = prepare(&mut p, api, args);
        let run = p.run(0);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, cpu);
        p.cpu.set_register(
            Register32::Esp,
            0x1001_0000 - u32::try_from(args.len()).unwrap() * 4,
        );
        let cpu = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, cpu);
    }
}
