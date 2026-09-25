use super::window_creation_executable;

use ring3_core::execution::{
    Access, Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const GET: u32 = 0x7000_0448;
const MESSAGE: u32 = 0x1000_c000;
const STACK: u32 = 0x1000_ef00;
const WINDOW: u32 = 0x7500_0004;

fn created() -> Process32 {
    let mut p = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), WINDOW);
    p
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
    p.cpu
}

#[test]
fn empty_get_message_waits_without_consuming_or_changing_guest_state() {
    let mut p = created();
    p.memory.write(u64::from(MESSAGE), &[0x5a; 32]).unwrap();
    prepare(&mut p, 0x7000_0000, &[77]);
    assert_eq!(p.run(1).api_calls, 1);
    for filter in [0, u32::MAX, WINDOW] {
        let before = prepare(&mut p, GET, &[MESSAGE, filter, 0, 0]);
        for _ in 0..2 {
            let run = p.run(1);
            assert_eq!(run.reason, ProcessStop::WaitingForMessage);
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            let mut bytes = [0; 32];
            p.memory.read(u64::from(MESSAGE), &mut bytes).unwrap();
            assert_eq!(bytes, [0x5a; 32]);
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
}

#[test]
fn unsupported_filters_and_inaccessible_message_do_not_wait() {
    let mut p = created();
    for args in [
        [MESSAGE, 0xdead_beef, 0, 0],
        [MESSAGE, 0, 0x1_0000, 0],
        [MESSAGE, 0, 0, 0x1_0000],
    ] {
        let before = prepare(&mut p, GET, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: GET });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    let before = prepare(&mut p, GET, &[MESSAGE, 0, 0, 0]);
    p.memory
        .protect(u64::from(MESSAGE), 4096, Permissions::READ)
        .unwrap();
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(MESSAGE),
            access: Access::Write,
        }))
    );
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}
