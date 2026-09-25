use super::window_creation_executable;

use ring3_core::execution::{
    Access, Cpu32, MemoryError, Permissions, PostMessageError, PostedMessage, Process32,
    ProcessStop, Register32, StopReason,
};

const PEEK: u32 = 0x7000_043c;
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

fn posted(hwnd: u32, message: u32, wparam: u32) -> PostedMessage {
    PostedMessage {
        hwnd,
        message,
        wparam,
        lparam: 0x1020_3040,
        time: 1234,
        point: [12, -5],
    }
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

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    p.cpu.register(Register32::Eax)
}

fn message_words(p: &Process32) -> [u32; 8] {
    let mut bytes = [0; 32];
    p.memory.read(u64::from(MESSAGE), &mut bytes).unwrap();
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    })
}

#[test]
fn peek_filter_and_get_deliver_host_messages_without_reordering() {
    let mut p = created();
    p.post_message(posted(WINDOW, 0x401, 11)).unwrap();
    p.post_message(posted(WINDOW, 0x402, 22)).unwrap();
    p.memory.write(u64::from(MESSAGE), &[0x5a; 32]).unwrap();
    assert_eq!(call(&mut p, PEEK, &[MESSAGE, WINDOW, 0, 0, 0]), 1);
    assert_eq!(
        message_words(&p),
        [
            WINDOW,
            0x401,
            11,
            0x1020_3040,
            1234,
            12,
            (-5_i32).cast_unsigned(),
            0
        ]
    );
    assert_eq!(call(&mut p, PEEK, &[MESSAGE, WINDOW, 0x402, 0x402, 1]), 1);
    assert_eq!(message_words(&p)[1..3], [0x402, 22]);
    assert_eq!(call(&mut p, GET, &[MESSAGE, WINDOW, 0, 0]), 1);
    assert_eq!(message_words(&p)[1..3], [0x401, 11]);
    let before = prepare(&mut p, GET, &[MESSAGE, 0, 0, 0]);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::WaitingForMessage);
    assert_eq!(p.cpu, before);
}

#[test]
fn thread_message_and_quit_have_distinct_get_results() {
    let mut p = created();
    p.post_message(posted(0, 0x403, 33)).unwrap();
    assert_eq!(call(&mut p, GET, &[MESSAGE, u32::MAX, 0, 0]), 1);
    assert_eq!(message_words(&p)[..3], [0, 0x403, 33]);
    p.post_message(posted(0, 0x12, 44)).unwrap();
    p.post_message(posted(WINDOW, 0x404, 55)).unwrap();
    assert_eq!(call(&mut p, GET, &[MESSAGE, 0, 0, 0]), 1);
    assert_eq!(message_words(&p)[..3], [WINDOW, 0x404, 55]);
    assert_eq!(call(&mut p, GET, &[MESSAGE, 0, 0x400, 0x4ff]), 0);
    assert_eq!(message_words(&p)[..3], [0, 0x12, 44]);
}

#[test]
fn rejected_posts_and_guest_write_fault_preserve_the_queue() {
    let mut p = created();
    assert_eq!(
        p.post_message(posted(0xdead_beef, 0x401, 1)),
        Err(PostMessageError::InvalidWindow)
    );
    assert_eq!(
        p.post_message(posted(WINDOW, 0x1_0000, 1)),
        Err(PostMessageError::InvalidMessage)
    );
    p.post_message(posted(WINDOW, 0x401, 55)).unwrap();
    let before = prepare(&mut p, GET, &[MESSAGE, WINDOW, 0, 0]);
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
    p.memory
        .protect(u64::from(MESSAGE), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, GET, &[MESSAGE, WINDOW, 0, 0]), 1);
    assert_eq!(message_words(&p)[2], 55);

    let mut full = created();
    for _ in 0..1024 {
        full.post_message(posted(WINDOW, 0x401, 1)).unwrap();
    }
    assert_eq!(
        full.post_message(posted(WINDOW, 0x401, 1)),
        Err(PostMessageError::Full)
    );
}
