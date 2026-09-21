#[path = "support/clipboard_formats_executable.rs"]
mod clipboard_formats_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const MESSAGE: u32 = 0x7000_0094;
const CLIPBOARD: u32 = 0x7000_00c8;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "uSeR32.dll",
            &["RegisterWindowMessageA", "RegisterClipboardFormatA"],
        ),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, api: u32, pointer: u32) -> Cpu32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK + 4), &pointer.to_le_bytes())
        .unwrap();
    process.cpu
}

fn register(process: &mut Process32, api: u32, text: &[u8]) -> u32 {
    process.memory.write(u64::from(NAME), text).unwrap();
    let before = prepare(process, api, NAME);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    let mut expected = before;
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 8);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
    value
}

#[test]
fn formats_and_messages_share_owned_names_but_not_other_guest_sessions() {
    let mut first = process();
    first
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(register(&mut first, MESSAGE, b"alpha\0"), 0xc000);
    assert_eq!(register(&mut first, CLIPBOARD, b"ALPHA\0"), 0xc000);
    assert_eq!(register(&mut first, CLIPBOARD, b"beta\0"), 0xc001);
    assert_eq!(register(&mut first, MESSAGE, b"BeTa\0"), 0xc001);
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(first.last_error().unwrap(), 77);
    let mut second = process();
    assert_eq!(register(&mut second, CLIPBOARD, b"beta\0"), 0xc000);
    assert_eq!(register(&mut first, CLIPBOARD, b"alpha\0"), 0xc000);
}

#[test]
fn clipboard_faults_and_unsupported_names_do_not_consume_shared_slots() {
    let mut process = process();
    for text in [b"\0".as_slice(), b"#12\0", b"\x80\0", &[b'x'; 256]] {
        process.memory.write(u64::from(NAME), text).unwrap();
        let before = prepare(&mut process, CLIPBOARD, NAME);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: CLIPBOARD }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for pointer in [0, 1, 0xffff] {
        let before = prepare(&mut process, CLIPBOARD, pointer);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: CLIPBOARD }
        );
        assert_eq!(process.cpu, before);
    }
    for (pointer, stack) in [(0x0040_3000, STACK), (NAME, 0x1000_fffc)] {
        prepare(&mut process, CLIPBOARD, pointer);
        process.cpu.set_register(Register32::Esp, stack);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    assert_eq!(register(&mut process, CLIPBOARD, b"first\0"), 0xc000);
    assert_eq!(register(&mut process, MESSAGE, b"FIRST\0"), 0xc000);
    assert_eq!(process.last_error().unwrap(), 0);
}

#[test]
fn formats_and_messages_exhaust_one_shared_capacity_atomically() {
    let mut process = process();
    for index in 0..0x4000_u32 {
        let api = if index % 2 == 0 { MESSAGE } else { CLIPBOARD };
        assert_eq!(
            register(&mut process, api, format!("name.{index}\0").as_bytes()),
            0xc000 + index
        );
    }
    process
        .memory
        .write(u64::from(NAME), b"overflow\0")
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, CLIPBOARD, NAME);
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(register(&mut process, CLIPBOARD, b"NAME.0\0"), 0xc000);
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for api in [MESSAGE, CLIPBOARD] {
        assert_eq!(register(&mut process, api, b"overflow\0"), 0);
        assert_eq!(process.last_error().unwrap(), 8);
        assert_eq!(register(&mut process, api, b"name.16383\0"), 0xffff);
    }
}

#[test]
fn imported_clipboard_guest_resumes_with_shared_message_identity() {
    let bytes = clipboard_formats_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 3));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..30 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (9, 3));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(whole.cpu.register(Register32::Eax), 0xc001);
    assert_eq!(whole.cpu.register(Register32::Ebx), 0xc000);
    assert_eq!(whole.cpu.register(Register32::Ecx), 0xc000);
}
