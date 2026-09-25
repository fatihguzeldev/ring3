use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0094;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "uSeR32.dll", &["RegisterWindowMessageA"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, pointer: u32) -> Cpu32 {
    process.cpu.eip = API;
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

fn register(process: &mut Process32, pointer: u32) -> u32 {
    let before = prepare(process, pointer);
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

fn name(process: &mut Process32, bytes: &[u8]) {
    process.memory.write(u64::from(NAME), bytes).unwrap();
}

#[test]
fn message_names_are_owned_case_insensitive_and_isolated_per_guest_session() {
    let mut first = process();
    first
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (text, id) in [
        (b"Ring3.Alpha\0".as_slice(), 0xc000),
        (b"RING3.ALPHA\0", 0xc000),
        (b"Ring3.Beta\0", 0xc001),
        (b"ring3.alpha\0ignored", 0xc000),
    ] {
        name(&mut first, text);
        assert_eq!(register(&mut first, NAME), id);
    }
    first
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(first.last_error().unwrap(), 77);
    let mut second = process();
    name(&mut second, b"Ring3.Beta\0");
    assert_eq!(register(&mut second, NAME), 0xc000);
    name(&mut first, b"Ring3.Beta\0");
    assert_eq!(register(&mut first, NAME), 0xc001);
}

#[test]
fn message_strings_stop_at_nul_across_pages_and_at_the_address_limit() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for address in [0x0040_2ffe, 0xffff_fffd] {
        process.memory.write(address, b"ab\0").unwrap();
        assert_eq!(
            register(&mut process, u32::try_from(address).unwrap()),
            0xc000
        );
    }
    process
        .memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process.memory.write(0x0040_2ffd, b"ab\0").unwrap();
    assert_eq!(register(&mut process, 0x0040_2ffd), 0xc000);
    process.memory.write(0x0040_2180, &[b'x'; 255]).unwrap();
    process.memory.write(0x0040_227f, &[0]).unwrap();
    assert_eq!(register(&mut process, NAME), 0xc001);
}

#[test]
fn unsupported_or_faulting_names_do_not_consume_message_identifiers() {
    let mut process = process();
    for text in [
        b"\0".as_slice(),
        b"#12\0",
        b"#named\0",
        b"\x80\0",
        &[b'x'; 256],
    ] {
        name(&mut process, text);
        let before = prepare(&mut process, NAME);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for pointer in [0, 1, 0xbfff, 0xffff] {
        let before = prepare(&mut process, pointer);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(process.cpu, before);
    }
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0xffff_ffff, b"a").unwrap();
    process.memory.write(0x0040_2fff, b"a").unwrap();
    for pointer in [0x0040_2fff, 0x0040_3000, 0xffff_ffff] {
        let before = prepare(&mut process, pointer);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    name(&mut process, b"first\0");
    assert_eq!(register(&mut process, NAME), 0xc000);
    assert_eq!(process.last_error().unwrap(), 0);
}

#[test]
fn message_capacity_is_bounded_and_existing_names_survive_exhaustion() {
    let mut process = process();
    for index in 0..0x4000_u32 {
        name(&mut process, format!("message.{index}\0").as_bytes());
        assert_eq!(register(&mut process, NAME), 0xc000 + index);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    name(&mut process, b"overflow\0");
    let before = prepare(&mut process, NAME);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(process.last_error().unwrap(), 0);
    name(&mut process, b"MESSAGE.0\0");
    assert_eq!(register(&mut process, NAME), 0xc000);
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    name(&mut process, b"overflow\0");
    assert_eq!(register(&mut process, NAME), 0);
    assert_eq!(process.last_error().unwrap(), 8);
    name(&mut process, b"message.16383\0");
    assert_eq!(register(&mut process, NAME), 0xffff);
    assert_eq!(process.last_error().unwrap(), 8);
    name(&mut process, b"overflow\0");
    process
        .memory
        .write(0x7ffd_e034, &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(0x7ffd_e038, &NAME.to_le_bytes())
        .unwrap();
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, 0x7ffd_e034);
    let mut expected = process.cpu;
    expected.eip = 8;
    expected.set_register(Register32::Esp, 0x7ffd_e03c);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu, expected);
}

#[test]
fn message_registration_requires_a_readable_frame_and_only_reads_the_name() {
    let mut process = process();
    name(&mut process, b"first\0");
    prepare(&mut process, NAME);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(register(&mut process, NAME), 0xc000);
    let mut text = [0; 6];
    process.memory.read(u64::from(NAME), &mut text).unwrap();
    assert_eq!(&text, b"first\0");
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut process, NAME);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}

use super::messages_executable;

#[test]
fn imported_message_guest_resumes_with_the_same_registry() {
    let bytes = messages_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
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
    assert_eq!(whole.cpu.register(Register32::Esp), stack);
    assert_eq!(whole.cpu.register(Register32::Eax), 0xc001);
    assert_eq!(whole.cpu.register(Register32::Ebx), 0xc000);
    assert_eq!(whole.cpu.register(Register32::Ecx), 0xc000);
}
