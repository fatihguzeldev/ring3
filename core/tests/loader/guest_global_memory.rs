use super::global_memory_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn guest_global_memory_lifecycle_has_single_step_parity() {
    let bytes = global_memory_executable::pe32();
    let mut whole = Process32::load(&bytes, 27).unwrap();
    let mut stepped = Process32::load(&bytes, 27).unwrap();
    let pages = whole.memory.mapped_pages();
    let run = whole.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(run.api_calls, 4);
    let (mut instructions, mut api_calls) = (0, 0);
    for _ in 0..100 {
        let step = stepped.run(1);
        instructions += step.instructions;
        api_calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, run.reason);
            break;
        }
    }
    assert_eq!((instructions, api_calls), (run.instructions, run.api_calls));
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Ebx), 42);
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.last_error().unwrap(), 0);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert!(
            process
                .memory
                .read(u64::from(process.cpu.register(Register32::Edi)), &mut [0])
                .is_err()
        );
    }
}
