#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const DISPATCH: u32 = 0x7000_0450;
const MESSAGE: u32 = 0x1000_c000;
const STACK: u32 = 0x1000_ef00;
const WINDOW: u32 = 0x7500_0004;

fn created() -> Process32 {
    let mut procedure = vec![
        0x81, 0x7c, 0x24, 8, 0, 4, 0, 0, 0x75, 8, 0xb8, 42, 0, 0, 0, 0xc2, 16, 0,
    ];
    procedure.extend(window_creation_executable::default_procedure());
    let mut bytes = window_creation_executable::guest();
    bytes[0x300..0x300 + procedure.len()].copy_from_slice(&procedure);
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Ebx), WINDOW);
    process
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) -> ring3_core::execution::Cpu32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    process.cpu
}

fn write_message(process: &mut Process32, words: [u32; 8]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    process.memory.write(u64::from(MESSAGE), &bytes).unwrap();
}

fn read_message(process: &Process32) -> [u32; 8] {
    let mut bytes = [0; 32];
    process.memory.read(u64::from(MESSAGE), &mut bytes).unwrap();
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    })
}

#[test]
fn owned_window_message_calls_current_procedure_and_returns_result() {
    let mut process = created();
    let words = [WINDOW, 0x400, 10, 32, 1234, 12, 34, 0];
    let before_error = process.last_error().unwrap();
    write_message(&mut process, words);
    prepare(&mut process, DISPATCH, &[MESSAGE]);
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 42);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 8);
    assert_eq!(process.last_error().unwrap(), before_error);
    assert_eq!(read_message(&process), words);
}

#[test]
fn thread_message_has_no_window_callback() {
    let mut process = created();
    let words = [0, 0x401, 10, 32, 1234, 12, 34, 0];
    write_message(&mut process, words);
    prepare(&mut process, DISPATCH, &[MESSAGE]);
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 8);
    assert_eq!(read_message(&process), words);
}

#[test]
fn invalid_message_and_callback_stack_reject_before_guest_effects() {
    let mut process = created();
    for words in [
        [0xdead_beef, 0x400, 10, 32, 0, 0, 0, 0],
        [WINDOW, 0x113, 10, 0x0040_1000, 0, 0, 0, 0],
    ] {
        write_message(&mut process, words);
        let before = prepare(&mut process, DISPATCH, &[MESSAGE]);
        let run = process.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi { address: DISPATCH }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(read_message(&process), words);
    }
    let before = prepare(&mut process, DISPATCH, &[0]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: 0
        }))
    );
    assert_eq!(process.cpu, before);
    write_message(&mut process, [WINDOW, 0x400, 10, 32, 0, 0, 0, 0]);
    let before = prepare(&mut process, DISPATCH, &[MESSAGE]);
    process
        .memory
        .protect(0x1000_e000, 4096, Permissions::READ)
        .unwrap();
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(read_message(&process)[1], 0x400);
}
