#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/resumed_thread_cases.rs"]
mod resumed_thread_cases;

#[test]
fn resumed_guest_preserves_context_across_host_budgets() {
    resumed_thread_cases::resumed_guest_preserves_context_across_host_budgets();
}

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const CODE: u32 = 0x0040_1000;
const DATA: u32 = 0x0040_2180;
const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;
const CURRENT: u32 = u32::MAX - 1;

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(p: &mut Process32, offset: u32, args: &[u32]) -> Cpu32 {
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

fn call(p: &mut Process32, offset: u32, args: &[u32]) -> u32 {
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

fn spin() -> Process32 {
    let mut code = vec![0xeb, 0xfe];
    code.resize(16, 0xcc);
    code.extend_from_slice(&[0x8b, 0x44, 0x24, 4, 0xff, 0x00, 0xeb, 0xfc]);
    let mut p = Process32::load(
        &imported_executable::pe32(&code, "kernel32.dll", &["ResumeThread"]),
        96,
    )
    .unwrap();
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
    p
}

fn create(p: &mut Process32, argument: u32) -> u32 {
    call(p, 0x548, &[0, 0, CODE + 16, argument, 4, 0])
}

#[test]
fn equal_priority_threads_rotate_with_closed_handles_and_zero_budget_keeps_pending_yield() {
    let mut p = spin();
    let first = create(&mut p, DATA);
    let second = create(&mut p, DATA + 4);
    assert_eq!(call(&mut p, 0x550, &[first]), 1);
    assert_eq!(call(&mut p, 0x550, &[first]), 0);
    assert_eq!(call(&mut p, 0x550, &[CURRENT]), 0);
    assert_eq!(call(&mut p, 0x550, &[second]), 1);
    assert_eq!(call(&mut p, 0x21c, &[first]), 1);
    assert_eq!(call(&mut p, 0x550, &[first]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    let before = p.cpu;
    let run = p.run(0);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    for _ in 0..4096 {
        p.run(1);
        if p.cpu.fs_base() == CHILD {
            break;
        }
    }
    assert_eq!(p.cpu.fs_base(), CHILD);
    for (index, (teb, counts)) in [
        (CHILD, (2048, 0)),
        (CHILD + 0x11000, (2048, 2048)),
        (PRIMARY, (2048, 2048)),
        (CHILD, (4096, 2048)),
    ]
    .into_iter()
    .enumerate()
    {
        let budget = if index == 0 { 4095 } else { 4096 };
        let run = p.run(budget);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (budget, 0));
        assert_eq!(p.cpu.fs_base(), teb);
        assert_eq!((word(&p, DATA), word(&p, DATA + 4)), counts);
    }
}

#[test]
fn lowering_the_active_priority_selects_the_highest_ready_thread_after_the_api_return() {
    let mut p = spin();
    let first = create(&mut p, DATA);
    let second = create(&mut p, DATA + 4);
    assert_eq!(call(&mut p, 0x254, &[first, 1]), 1);
    assert_eq!(call(&mut p, 0x550, &[first]), 1);
    assert_eq!(call(&mut p, 0x550, &[second]), 1);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
    p.run(8192);
    assert_eq!(p.cpu.fs_base(), CHILD);
    assert_eq!(word(&p, DATA + 4), 0);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
    let before = p.cpu;
    p.run(0);
    assert_eq!(p.cpu, before);
    p.run(1);
    assert_eq!(p.cpu.fs_base(), CHILD + 0x11000);
    assert_eq!(word(&p, DATA + 4), 0);
    p.run(1);
    assert_eq!(word(&p, DATA + 4), 1);
}

#[test]
fn resume_failures_preserve_cpu_and_thread_state_when_last_error_is_read_only() {
    let mut p = spin();
    let closed = create(&mut p, DATA);
    let child = create(&mut p, DATA + 4);
    let mutex = call(&mut p, 0x210, &[0, 0, 0]);
    assert_eq!(call(&mut p, 0x21c, &[closed]), 1);
    for teb in [CHILD, CHILD + 0x11000] {
        p.cpu.set_fs_base(teb);
        let before = prepare(&mut p, 0x550, &[child]);
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
    p.cpu.set_fs_base(PRIMARY);
    put(&mut p, PRIMARY + 0x34, 77);
    p.memory
        .protect(u64::from(PRIMARY), 4096, Permissions::READ)
        .unwrap();
    for target in [0, u32::MAX, closed, mutex] {
        let before = prepare(&mut p, 0x550, &[target]);
        for _ in 0..2 {
            let run = p.run(1);
            assert!(matches!(
                run.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!(word(&p, PRIMARY + 0x34), 77);
        }
    }
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    assert_eq!(call(&mut p, 0x550, &[child]), 0);
    assert_eq!(word(&p, PRIMARY + 0x34), 77);
}

#[test]
fn scheduled_boundaries_reject_foreign_fs_and_child_gui_before_reading_arguments() {
    let mut p = spin();
    let first = create(&mut p, DATA);
    let _second = create(&mut p, DATA + 4);
    assert_eq!(call(&mut p, 0x550, &[first]), 1);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    for _ in 0..4096 {
        p.run(1);
        if p.cpu.fs_base() == CHILD {
            break;
        }
    }
    assert_eq!(p.cpu.fs_base(), CHILD);
    for teb in [PRIMARY, CHILD + 0x11000, 0] {
        p.cpu.set_fs_base(teb);
        let before = p.cpu;
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
    p.cpu.set_fs_base(CHILD);
    let pages = p.memory.mapped_pages();
    for offset in [0x464, 0x2a8, 0x474, 12, 0xa0, 0xcc, 16] {
        p.cpu.eip = 0x7000_0000 + offset;
        p.cpu.set_register(Register32::Esp, 0);
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert!(p.window_snapshots().is_empty());
        assert!(p.take_frame().is_none());
    }
}

#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;

#[test]
fn dll_startup_deferred_attach_and_child_notifications_pin_the_owner_across_quanta() {
    use ring3_core::execution::{GuestModule, ProcessOptions};
    let mut dll_code = vec![0xb9];
    dll_code.extend_from_slice(&9000_u32.to_le_bytes());
    dll_code.extend_from_slice(&[0x49, 0x75, 0xfd, 0x64, 0xa1, 0x24, 0, 0, 0, 0xa3]);
    dll_code.extend_from_slice(&(DATA + 8).to_le_bytes());
    dll_code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    let dll = dll_executable::dll(0x5000_0000, &dll_code, None);
    let providers = [GuestModule {
        name: "long.dll",
        bytes: &dll,
    }];
    let exe = imported_executable::pe32(&[0xeb, 0xfe], "kernel32.dll", &["ResumeThread"]);
    for deferred in [false, true] {
        let mut p = Process32::load_with_options(
            &exe,
            96,
            ProcessOptions {
                modules: if deferred { &[] } else { &providers },
                deferred_modules: if deferred { &providers } else { &[] },
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let startup_cpu = p.cpu;
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        let handle = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
        assert_eq!(call(&mut p, 0x550, &[handle]), 1);
        if deferred {
            p.memory.write(u64::from(DATA + 32), b"long.dll\0").unwrap();
            prepare(&mut p, 0x14, &[DATA + 32]);
            let run = p.run(1);
            assert_eq!((run.instructions, run.api_calls), (0, 1));
            assert_eq!(p.cpu.eip, 0x5000_1000);
        } else {
            p.cpu = startup_cpu;
        }
        let pending = p.cpu;
        assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
        p.cpu = pending;
        for _ in 0..4 {
            let run = p.run(4096);
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert_eq!(p.cpu.fs_base(), PRIMARY);
            assert_eq!(word(&p, DATA + 8), 0);
        }
        for _ in 0..4096 {
            p.run(1);
            if p.cpu.fs_base() == CHILD {
                break;
            }
        }
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(word(&p, DATA + 8), 1);
        let pending = p.cpu;
        assert_eq!(
            call(&mut p, 0x254, &[CURRENT, (-15_i32).cast_unsigned()]),
            1
        );
        p.cpu = pending;
        for _ in 0..4 {
            let run = p.run(4096);
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert_eq!(p.cpu.fs_base(), CHILD);
            assert_eq!(word(&p, DATA + 8), 1);
        }
        for _ in 0..4096 {
            p.run(1);
            if p.cpu.fs_base() == PRIMARY {
                break;
            }
        }
        assert_eq!(p.cpu.fs_base(), PRIMARY);
        assert_eq!(word(&p, DATA + 8), 2);
    }
}

#[test]
fn a_scheduled_api_fault_does_not_spend_the_remaining_quantum() {
    let mut p = spin();
    let child = create(&mut p, DATA);
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    for _ in 0..4096 {
        p.run(1);
        if p.cpu.fs_base() == CHILD {
            break;
        }
    }
    assert_eq!(p.cpu.fs_base(), CHILD);
    put(&mut p, CHILD + 0x34, 77);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, 0x550, &[0]);
    for _ in 0..2 {
        let run = p.run(10000);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, CHILD + 0x34), 77);
    }
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, 0x550, &[0]), u32::MAX);
    assert_eq!(word(&p, CHILD + 0x34), 6);
    assert_eq!(word(&p, PRIMARY + 0x34), 0);
    assert_eq!(p.run(4094).instructions, 4094);
    assert_eq!(p.cpu.fs_base(), CHILD);
    assert_eq!(p.run(1).instructions, 1);
    assert_eq!(p.cpu.fs_base(), PRIMARY);
}
