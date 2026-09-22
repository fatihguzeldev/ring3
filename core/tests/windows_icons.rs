#[path = "support/icon_executable.rs"]
mod icon_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_icon_load_reuses_the_selected_resource_across_budgets() {
    let mut expected = None;
    for budget in [1, 100] {
        let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(p.cpu.register(Register32::Eax), 0x7800_0004);
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7800_0004);
        if let Some(previous) = expected {
            assert_eq!((p.cpu, counts), previous);
        }
        expected = Some((p.cpu, counts));
    }
}
