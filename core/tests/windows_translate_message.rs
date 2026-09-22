#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{
    MemoryError, PostedMessage, Process32, ProcessStop, Register32, StopReason,
};

const TRANSLATE: u32 = 0x7000_044c;
const GET: u32 = 0x7000_0448;
const MESSAGE: u32 = 0x1000_c000;
const OUTPUT: u32 = MESSAGE + 64;
const STACK: u32 = 0x1000_ef00;
const WINDOW: u32 = 0x7500_0004;

fn created() -> Process32 {
    let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(
        process.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    process
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    let words: Vec<_> = std::iter::once(&0x0040_10f0_u32)
        .chain(args)
        .flat_map(|word| word.to_le_bytes())
        .collect();
    process.memory.write(u64::from(STACK), &words).unwrap();
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(process, api, args);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    process.cpu.register(Register32::Eax)
}

fn message(process: &Process32, address: u32) -> [u32; 8] {
    let mut bytes = [0; 32];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    })
}

fn write_message(process: &mut Process32, words: [u32; 8]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    process.memory.write(u64::from(MESSAGE), &bytes).unwrap();
}

#[test]
fn return_key_posts_character_without_mutating_input() {
    let mut process = created();
    let key = [
        WINDOW,
        0x100,
        13,
        0x001c_0001,
        1234,
        12,
        (-5_i32).cast_unsigned(),
        0,
    ];
    write_message(&mut process, key);
    assert_eq!(call(&mut process, TRANSLATE, &[MESSAGE]), 1);
    assert_eq!(message(&process, MESSAGE), key);
    assert_eq!(call(&mut process, GET, &[OUTPUT, WINDOW, 0, 0]), 1);
    assert_eq!(
        message(&process, OUTPUT),
        [
            WINDOW,
            0x102,
            13,
            0x001c_0001,
            1234,
            12,
            (-5_i32).cast_unsigned(),
            0
        ]
    );
    let mut system_key = key;
    system_key[1] = 0x104;
    write_message(&mut process, system_key);
    assert_eq!(call(&mut process, TRANSLATE, &[MESSAGE]), 1);
    assert_eq!(call(&mut process, GET, &[OUTPUT, WINDOW, 0, 0]), 1);
    assert_eq!(message(&process, OUTPUT)[1], 0x106);
}

#[test]
fn noncharacter_messages_preserve_queue_and_invalid_pointer_stops() {
    let mut process = created();
    let mut words = [WINDOW, 0x101, 13, 0, 0, 0, 0, 0];
    write_message(&mut process, words);
    assert_eq!(call(&mut process, TRANSLATE, &[MESSAGE]), 1);
    words[1] = 0x401;
    write_message(&mut process, words);
    assert_eq!(call(&mut process, TRANSLATE, &[MESSAGE]), 0);
    prepare(&mut process, GET, &[OUTPUT, 0, 0, 0]);
    assert_eq!(process.run(1).reason, ProcessStop::WaitingForMessage);
    prepare(&mut process, TRANSLATE, &[0]);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: 0
        }))
    );
    assert_eq!(process.cpu, before);
}

#[test]
fn full_queue_rejects_character_without_changing_input() {
    let mut process = created();
    for _ in 0..1024 {
        process
            .post_message(PostedMessage {
                hwnd: WINDOW,
                message: 0x401,
                wparam: 0,
                lparam: 0,
                time: 0,
                point: [0, 0],
            })
            .unwrap();
    }
    let key = [WINDOW, 0x100, 13, 1, 0, 0, 0, 0];
    write_message(&mut process, key);
    prepare(&mut process, TRANSLATE, &[MESSAGE]);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: TRANSLATE }
    );
    assert_eq!(process.cpu, before);
    assert_eq!(message(&process, MESSAGE), key);
    assert_eq!(call(&mut process, GET, &[OUTPUT, WINDOW, 0, 0]), 1);
    assert_eq!(message(&process, OUTPUT)[1], 0x401);
}
