use super::window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const DESTROY: u32 = 0x7000_0470;
const STACK: u32 = 0x1000_ef00;

fn prepare(process: &mut Process32, hwnd: u32) {
    process.cpu.eip = DESTROY;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = [0x0040_10f0_u32, hwnd]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
}

#[test]
fn invalid_handle_fails_and_custom_window_is_unsupported() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    let window = process.window_snapshots()[0].clone();
    prepare(&mut process, 0);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    prepare(&mut process, hwnd);
    let cpu = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: DESTROY }
    );
    assert_eq!(process.cpu, cpu);
    assert_eq!(process.window_snapshots()[0], window);
}
