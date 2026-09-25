use super::window_creation_executable;

use std::time::Duration;

use ring3_core::execution::{PostedMessage, Process32, ProcessStop, Register32, StopReason};

const POST: u32 = 0x7000_0464;
const GET: u32 = 0x7000_0448;
const STACK: u32 = 0x1000_ef00;
const MESSAGE: u32 = 0x1000_c000;

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    process.cpu.register(Register32::Eax)
}

fn created() -> Process32 {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    process
}

#[test]
fn guest_post_delivers_the_existing_queue_message_with_timestamp() {
    let mut process = created();
    let hwnd = process.cpu.register(Register32::Ebx);
    process
        .set_elapsed_time(Duration::from_millis(1234))
        .unwrap();
    call(&mut process, 0x7000_0000, &[77]);
    assert_eq!(call(&mut process, POST, &[hwnd, 0x401, 10, 32]), 1);
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(call(&mut process, GET, &[MESSAGE, 0, 0, 0]), 1);
    let mut bytes = [0; 32];
    process.memory.read(u64::from(MESSAGE), &mut bytes).unwrap();
    let words: Vec<_> = bytes
        .chunks_exact(4)
        .map(|part| u32::from_le_bytes(part.try_into().unwrap()))
        .collect();
    assert_eq!(words, [hwnd, 0x401, 10, 32, 1234, 0, 0, 0]);
}

#[test]
fn invalid_target_and_full_queue_report_failure_without_enqueuing() {
    let mut process = created();
    let hwnd = process.cpu.register(Register32::Ebx);
    assert_eq!(call(&mut process, POST, &[0xdead_beef, 0x401, 0, 0]), 0);
    assert_eq!(process.last_error().unwrap(), 1400);
    for _ in 0..1024 {
        process
            .post_message(PostedMessage {
                hwnd,
                message: 0x401,
                wparam: 0,
                lparam: 0,
                time: 0,
                point: [0, 0],
            })
            .unwrap();
    }
    assert_eq!(call(&mut process, POST, &[hwnd, 0x402, 1, 2]), 0);
    assert_eq!(process.last_error().unwrap(), 1816);
}
