#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn host_snapshot_is_owned_detached_and_reflects_window_activation() {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert!(process.window_snapshots().is_empty());
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let hwnd = process.cpu.register(Register32::Ebx);
    let before = process.window_snapshots();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].hwnd, hwnd);
    assert_eq!(before[0].parent, 0);
    assert_eq!(before[0].id, 0);
    assert_eq!(before[0].title, "title");
    assert!(!before[0].active);
    assert_eq!(before[0].dialog_units, None);

    process.cpu.eip = 0x7000_0440;
    process.cpu.set_register(Register32::Esp, 0x1000_ef00);
    let frame: Vec<_> = [0x0040_10f0_u32, hwnd, 1]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(0x1000_ef00, &frame).unwrap();
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    let after = process.window_snapshots();
    assert_eq!(after.len(), 1);
    assert!(after[0].active);
    assert_ne!(after[0].style, before[0].style);
    assert!(!before[0].active);
}
