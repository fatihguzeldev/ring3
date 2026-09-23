#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const ENABLE: u32 = 0x7000_0460;
const STACK: u32 = 0x1000_ef00;

fn prepare(process: &mut Process32, hwnd: u32, enabled: u32) {
    process.cpu.eip = ENABLE;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = [0x0040_10f0_u32, hwnd, enabled]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
}

#[test]
fn invalid_handle_fails_and_custom_procedure_is_not_silently_skipped() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    let style = process.window_snapshots()[0].style;
    prepare(&mut process, 0, 0);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    prepare(&mut process, hwnd, 0);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: ENABLE }
    );
    assert_eq!(process.cpu, before);
    assert_eq!(process.window_snapshots()[0].style, style);
}
