use super::window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const GET: u32 = 0x7000_0458;
const STACK: u32 = 0x1000_ef00;

fn query(process: &mut Process32, hwnd: u32, relation: u32) -> u32 {
    let before = process.cpu;
    process.cpu.eip = GET;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = [0x0040_10f0_u32, hwnd, relation]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = process.cpu.register(Register32::Eax);
    process.cpu = before;
    result
}

#[test]
fn top_level_window_relations_and_invalid_handle() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    for relation in [0, 1] {
        assert_eq!(query(&mut process, hwnd, relation), hwnd);
    }
    for relation in [2, 3, 4, 5] {
        assert_eq!(query(&mut process, hwnd, relation), 0);
    }
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(query(&mut process, 0xdead_beef, 2), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    assert_eq!(query(&mut process, hwnd, 2), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
}
