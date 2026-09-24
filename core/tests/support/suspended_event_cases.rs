use ring3_core::execution::{ProcessStop, Register32, StopReason};

use super::event_wait_control::*;

pub fn suspended_event_waits_preserve_signal_and_handle_ownership() {
    for manual in [false, true] {
        for (released, reset) in [(false, false), (false, true), (true, true)] {
            let mut p = process();
            p.memory.write(u64::from(DATA), b"suspend\0").unwrap();
            let event = call(&mut p, 0x53c, &[0, u32::from(manual), 0, DATA]);
            let alias = call(&mut p, 0x53c, &[0, 0, 0, DATA]);
            let child = park_child(&mut p, event);
            assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
            if released {
                assert_eq!(call(&mut p, 0x540, &[alias]), 1);
            }
            assert_eq!(call(&mut p, 0x554, &[child]), 0);
            assert_eq!(call(&mut p, 0x554, &[child]), 1);
            if !released {
                assert_eq!(call(&mut p, 0x540, &[alias]), 1);
            }
            denied(&mut p, 0x21c, &[event]);
            assert_eq!(call(&mut p, 0x550, &[child]), 2);
            if reset {
                assert_eq!(call(&mut p, 0x544, &[alias]), 1);
            }
            // the original wait, not these edited arguments, owns completion.
            put(&mut p, CHILD - 16, 0);
            put(&mut p, CHILD - 12, 0);
            assert_eq!(call(&mut p, 0x550, &[child]), 1);
            denied(&mut p, 0x21c, &[event]);
            assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
            assert_eq!(
                p.run(100).reason,
                ProcessStop::Stopped(StopReason::Breakpoint)
            );
            if !released && reset {
                assert_eq!(p.cpu.fs_base(), PRIMARY);
                assert_eq!(call(&mut p, 0x540, &[alias]), 1);
                assert_eq!(
                    p.run(100).reason,
                    ProcessStop::Stopped(StopReason::Breakpoint)
                );
            }
            assert_eq!(p.cpu.fs_base(), CHILD);
            assert_eq!(p.cpu.register(Register32::Eax), 0);
            assert_eq!(p.cpu.register(Register32::Esp), CHILD - 8);
            assert_eq!(call(&mut p, 0x21c, &[event]), 1);
            assert_eq!(
                call(&mut p, 0x214, &[alias, 0]),
                if manual && !released { 0 } else { 258 }
            );
        }
    }
    for manual in [false, true] {
        let mut p = process();
        let event = call(&mut p, 0x53c, &[0, u32::from(manual), 0, 0]);
        let first = park_child(&mut p, event);
        park_child(&mut p, event);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        assert_eq!(call(&mut p, 0x554, &[first]), 0);
        assert_eq!(call(&mut p, 0x540, &[event]), 1);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.fs_base(), CHILD + 0x11000);
        denied(&mut p, 0x21c, &[event]);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        assert_eq!(call(&mut p, 0x550, &[first]), 1);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        if !manual {
            assert_eq!(p.cpu.fs_base(), PRIMARY);
            assert_eq!(call(&mut p, 0x540, &[event]), 1);
            assert_eq!(
                p.run(100).reason,
                ProcessStop::Stopped(StopReason::Breakpoint)
            );
        }
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(call(&mut p, 0x21c, &[event]), 1);
    }
}
