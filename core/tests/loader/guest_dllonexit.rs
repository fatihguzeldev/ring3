use super::dllonexit_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn guest_invokes_registered_functions_in_reverse_and_releases_table() {
    let bytes = dllonexit_executable::pe32();
    let mut whole = Process32::load(&bytes, 26).unwrap();
    let mut stepped = Process32::load(&bytes, 26).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
    let pages = whole.memory.mapped_pages();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 4);
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..100 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions, result.api_calls)
    );
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        assert_eq!(process.cpu.register(Register32::Edx), 0);
        assert_eq!(process.cpu.register(Register32::Ebx), 21);
        assert_eq!(process.memory.mapped_pages(), pages);
        let mut bytes = [0; 4];
        process.memory.read(0x0040_2190, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), 21);
    }
}
