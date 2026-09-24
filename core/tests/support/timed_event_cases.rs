use std::time::Duration;

use ring3_core::execution::{Process32, ProcessStop, StopReason};

use super::{event_wait_control::*, imported_executable};

pub fn timed_process(timeout: u32) -> Process32 {
    let mut code = vec![0xcc, 0xcc, 0xff, 0x05];
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0xeb, 0xf8]);
    code.resize(16, 0xcc);
    code.extend_from_slice(&[0x8b, 0x44, 0x24, 4, 0x68]);
    code.extend_from_slice(&timeout.to_le_bytes());
    code.extend_from_slice(&[0x50, 0xff, 0x15]);
    code.extend_from_slice(&0x0040_2060_u32.to_le_bytes());
    code.push(0xa3);
    code.extend_from_slice(&(DATA + 4).to_le_bytes());
    code.push(0xcc);
    Process32::load(
        &imported_executable::pe32(&code, "kernel32.dll", &["WaitForSingleObject"]),
        96,
    )
    .unwrap()
}

fn until_breakpoint(p: &mut Process32, budget: u64, counts: &mut (u64, u64)) {
    loop {
        let run = p.run(budget);
        counts.0 += run.instructions;
        counts.1 += run.api_calls;
        assert!(counts.0 < 50000);
        if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            return;
        }
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
}

fn work(p: &mut Process32, budget: u64, counts: &mut (u64, u64)) {
    let mut remaining = 8192;
    while remaining != 0 {
        let run = p.run(budget.min(remaining));
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        counts.0 += run.instructions;
        counts.1 += run.api_calls;
        assert_eq!(run.api_calls, 0);
        assert_ne!(run.instructions, 0);
        remaining -= run.instructions;
    }
}

pub fn finite_wait_uses_exact_host_time_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = timed_process(33);
        p.set_elapsed_time(Duration::from_micros(1500)).unwrap();
        let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
        let child = call(&mut p, 0x548, &[0, 0, CODE + 16, event, 4, 0]);
        assert_eq!(call(&mut p, 0x254, &[child, 1]), 1);
        assert_eq!(call(&mut p, 0x550, &[child]), 1);
        let mut counts = (0, 0);
        until_breakpoint(&mut p, budget, &mut counts);
        assert_eq!(p.cpu.fs_base(), PRIMARY);
        p.cpu.eip = CODE + 2;
        work(&mut p, budget, &mut counts);
        p.set_elapsed_time(Duration::from_nanos(34_499_999))
            .unwrap();
        work(&mut p, budget, &mut counts);
        assert_eq!(p.cpu.fs_base(), PRIMARY);
        p.set_elapsed_time(Duration::from_micros(34500)).unwrap();
        let before = p.cpu;
        let zero = p.run(0);
        assert_eq!((zero.instructions, zero.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        until_breakpoint(&mut p, budget, &mut counts);
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(counts.1, 1);
        let mut bytes = [0; 8];
        p.memory.read(u64::from(DATA), &mut bytes).unwrap();
        assert_eq!(&bytes[..4], &8192_u32.to_le_bytes());
        assert_eq!(&bytes[4..], &258_u32.to_le_bytes());
        results.push((p.cpu, counts, bytes));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
