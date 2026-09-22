#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;
use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const PEEK: u32 = 0x7000_043c;
const MESSAGE: u32 = 0x1000_c000;
const WINDOW: u32 = 0x7500_0004;
const STACK: u32 = 0x1000_ef00;

fn created() -> Process32 {
    let mut p = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Ebx), WINDOW);
    p
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> ring3_core::execution::Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x0040_10f0);
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK + u32::try_from(args.len() + 1).unwrap() * 4
    );
    p.cpu.register(Register32::Eax)
}

#[test]
fn empty_queue_preserves_message_and_last_error_for_supported_filters() {
    let mut p = created();
    p.memory.write(u64::from(MESSAGE), &[0x5a; 32]).unwrap();
    call(&mut p, 0x7000_0000, &[77]);
    for args in [
        [MESSAGE, 0, 0, 0, 0],
        [MESSAGE, u32::MAX, 0x100, 0x1ff, 1],
        [MESSAGE, WINDOW, 0x200, 0x20a, 3],
    ] {
        assert_eq!(call(&mut p, PEEK, &args), 0);
        let mut bytes = [0; 32];
        p.memory.read(u64::from(MESSAGE), &mut bytes).unwrap();
        assert_eq!(bytes, [0x5a; 32]);
        assert_eq!(p.last_error().unwrap(), 77);
    }
}

#[test]
fn unsupported_profiles_and_inaccessible_output_do_not_advance_api_state() {
    let mut p = created();
    for args in [
        [MESSAGE, 0xdead_beef, 0, 0, 0],
        [MESSAGE, 0, 0, 0, 4],
        [MESSAGE, 0, 0x1_0000, 0, 0],
    ] {
        let before = prepare(&mut p, PEEK, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: PEEK });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    let before = prepare(&mut p, PEEK, &[MESSAGE, 0, 0, 0, 0]);
    p.memory
        .protect(u64::from(MESSAGE), 4096, Permissions::READ)
        .unwrap();
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}
