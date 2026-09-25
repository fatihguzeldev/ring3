use super::window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const SET: u32 = 0x7000_045c;
const STACK: u32 = 0x1000_ef00;

fn set(process: &mut Process32, hwnd: u32, pointer: u32) -> ProcessStop {
    process.cpu.eip = SET;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = [0x0040_10f0_u32, hwnd, pointer]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    process.run(1).reason
}

#[test]
fn owned_window_text_changes_and_null_clears_it() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    assert_eq!(process.window_snapshots()[0].title, "title");
    process.memory.write(0x0040_2300, b"changed\0").unwrap();
    assert_eq!(
        set(&mut process, hwnd, 0x0040_2300),
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.window_snapshots()[0].title, "changed");
    assert_eq!(
        set(&mut process, hwnd, 0),
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert!(process.window_snapshots()[0].title.is_empty());
}

#[test]
fn invalid_handle_and_unreadable_title_do_not_mutate_window() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    assert_eq!(
        set(&mut process, 0xdead_beef, 0),
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    assert!(matches!(
        set(&mut process, hwnd, 0x5000_0000),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.window_snapshots()[0].title, "title");
}
