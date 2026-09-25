use ring3_core::execution::{Permissions, ProcessStop, Register32, StopReason};

use super::window_message_cases::{STACK, WINDOW, call, created, prepare};

const DEFAULT: u32 = 0x7000_02ac;

pub fn ordinary_keys_preserve_state() {
    let mut process = created();
    call(&mut process, 0x7000_0000, &[77]);
    let windows = process.window_snapshots();
    let pages = process.memory.mapped_pages();
    for message in [0x100, 0x101] {
        for key in (0..=255).filter(|key| !matches!(key, 0x12 | 0x5d | 0x79 | 0xa4 | 0xa5)) {
            for flags in [0, 0x0150_0001, 0xc150_0001, u32::MAX] {
                assert_eq!(
                    call(&mut process, DEFAULT, &[WINDOW, message, key, flags]),
                    0
                );
            }
        }
    }
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.window_snapshots(), windows);
    assert_eq!(process.memory.mapped_pages(), pages);
    process
        .memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, DEFAULT, &[WINDOW, 0x100, 40, 0]), 0);
}

pub fn special_keys_and_faults_remain_atomic() {
    let mut process = created();
    call(&mut process, 0x7000_0000, &[77]);
    for (message, key) in [
        (0x100, 0x79),
        (0x101, 0x79),
        (0x100, 0x12),
        (0x101, 0x12),
        (0x100, 0x5d),
        (0x101, 0xa4),
        (0x101, 0xa5),
        (0x100, 256),
        (0x101, u32::MAX),
        (0x104, 40),
        (0x105, 40),
        (0x102, 13),
        (0x106, 13),
    ] {
        let before = prepare(&mut process, DEFAULT, &[WINDOW, message, key, 0]);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: DEFAULT }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    prepare(&mut process, DEFAULT, &[WINDOW, 0x100, 40, 0]);
    process.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(call(&mut process, DEFAULT, &[0, 0x100, 40, 0]), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
}

pub fn default_key_callbacks_resume_whole_or_stepwise() {
    let mut expected = None;
    for budget in [1, 100] {
        let mut process = created();
        call(
            &mut process,
            0x7000_02c0,
            &[WINDOW, (-4_i32).cast_unsigned(), 0x0040_1100],
        );
        call(&mut process, 0x7000_0000, &[77]);
        let windows = process.window_snapshots();
        let mut counts = (0, 0);
        for (message, flags) in [(0x100, 0x0150_0001), (0x101, 0xc150_0001)] {
            prepare(&mut process, 0x7000_02cc, &[WINDOW, message, 40, flags]);
            loop {
                let result = process.run(budget);
                counts.0 += result.instructions;
                counts.1 += result.api_calls;
                if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 100);
            }
            assert_eq!(process.cpu.register(Register32::Eax), 0);
            assert_eq!(process.cpu.register(Register32::Esp), STACK + 20);
        }
        assert_eq!(counts.1, 4);
        assert_eq!(process.last_error().unwrap(), 77);
        assert_eq!(process.window_snapshots(), windows);
        if let Some(prior) = expected {
            assert_eq!((process.cpu, counts), prior);
        }
        expected = Some((process.cpu, counts));
    }
}
