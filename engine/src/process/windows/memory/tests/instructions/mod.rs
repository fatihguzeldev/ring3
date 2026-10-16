use super::*;
use crate::windows::WindowsApi32;

#[test]
fn instruction_flush_accepts_a_duplicated_current_process_and_rejects_foreign_handles() {
    let mut engine = engine();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::DuplicateHandle.id(),
            &[u32::MAX, u32::MAX, u32::MAX, 0x1800, 0, 0, 2]
        ),
        1
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1800), &mut bytes)
        .unwrap();
    let process = u32::from_le_bytes(bytes);
    assert_ne!(process, u32::MAX);
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::FlushInstructionCache.id(),
            &[process, 0x3000, 2]
        ),
        1
    );
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::FlushInstructionCache.id(),
            &[0xdeadbeef, 0, 0]
        ),
        0
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 6);
}
