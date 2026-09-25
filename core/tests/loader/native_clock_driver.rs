use super::performance_clock_executable;
#[path = "../../examples/support/timed.rs"]
mod timed;

use std::time::Duration;

use ring3_core::execution::{ClockError, Process32, ProcessStop, Register32, StopReason};

fn load() -> Process32 {
    Process32::load(&performance_clock_executable::pe32(), 32).unwrap()
}

#[test]
fn samples_each_work_unit_and_keeps_the_same_clock_across_resumption() {
    let mut final_cpu = None;
    for budgets in [&[50][..], &[6, 5][..]] {
        let mut p = load();
        let (mut ticks, mut instructions, mut calls) = (0, 0, 0);
        for &budget in budgets {
            let result = timed::run(&mut p, budget, || {
                ticks += 100;
                Duration::from_nanos(ticks)
            })
            .unwrap();
            instructions += result.instructions;
            calls += result.api_calls;
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(if ticks == 1100 {
                    StopReason::Breakpoint
                } else {
                    StopReason::InstructionLimit
                })
            );
        }
        assert_eq!((ticks, instructions, calls), (1100, 8, 3));
        let mut bytes = [0; 24];
        p.memory.read(0x0040_2280, &mut bytes).unwrap();
        for (chunk, value) in bytes.chunks_exact(8).zip([1_000_000_000_u64, 700, 1000]) {
            assert_eq!(chunk, value.to_le_bytes());
        }
        if let Some(cpu) = final_cpu {
            assert_eq!(p.cpu, cpu);
        }
        final_cpu = Some(p.cpu);
    }
}

#[test]
fn clock_failure_retains_completed_work_and_rejects_the_next_unit() {
    for invalid in [Duration::ZERO, Duration::MAX] {
        let mut p = load();
        let mut reference = load();
        reference
            .set_elapsed_time(Duration::from_nanos(100))
            .unwrap();
        let prior = reference.run(3);
        let mut samples = 0;
        let failure = timed::run(&mut p, 50, || {
            samples += 1;
            if samples <= 3 {
                Duration::from_nanos(100)
            } else {
                invalid
            }
        })
        .unwrap_err();
        assert_eq!(
            failure.error,
            if invalid.is_zero() {
                ClockError::WentBackwards
            } else {
                ClockError::OutOfRange
            }
        );
        assert_eq!(samples, 4);
        assert_eq!(failure.completed, prior);
        assert_eq!((prior.instructions, prior.api_calls), (2, 1));
        assert_eq!(p.cpu, reference.cpu);
        assert_eq!(
            p.set_elapsed_time(Duration::from_nanos(99)),
            Err(ClockError::WentBackwards)
        );
    }
}

#[test]
fn zero_budget_and_existing_exit_do_not_sample_and_faults_stop_immediately() {
    let mut p = load();
    let before = p.cpu;
    let result = timed::run(&mut p, 0, || panic!("zero budget sampled the clock")).unwrap();
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);

    p.cpu.eip = 0;
    let mut samples = 0;
    let result = timed::run(&mut p, 50, || {
        samples += 1;
        Duration::ZERO
    })
    .unwrap();
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((samples, result.instructions, result.api_calls), (1, 0, 0));

    p.cpu.eip = 0x7000_0008;
    p.cpu.set_register(Register32::Esp, 0x1000_ff00);
    p.memory
        .write(0x1000_ff00, &[0, 0, 0, 0, 42, 0, 0, 0])
        .unwrap();
    assert_eq!(p.run(1).reason, ProcessStop::Exited(42));
    let result = timed::run(&mut p, 50, || panic!("terminal process sampled the clock")).unwrap();
    assert_eq!(result.reason, ProcessStop::Exited(42));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
}
