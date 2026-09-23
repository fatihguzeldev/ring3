#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const SET_WINDOW_POS: u32 = 0x7000_046c;
const STACK: u32 = 0x1000_ef00;

fn prepare(process: &mut Process32, args: [u32; 7]) {
    process.cpu.eip = SET_WINDOW_POS;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(0x0040_10f0_u32)
        .chain(args)
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
}

#[test]
fn invalid_handle_fails_and_other_window_or_flag_profile_stops() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    let before_window = process.window_snapshots()[0].clone();
    prepare(&mut process, [0, 0, 0, 0, 0, 0, 0x97]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    for args in [[hwnd, 0, 0, 0, 0, 0, 0x97], [hwnd, 0, 0, 0, 0, 0, 0x96]] {
        prepare(&mut process, args);
        let before_cpu = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi {
                address: SET_WINDOW_POS
            }
        );
        assert_eq!(process.cpu, before_cpu);
        assert_eq!(process.window_snapshots()[0], before_window);
    }
}
