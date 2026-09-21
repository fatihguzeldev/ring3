#[path = "support/command_line_executable.rs"]
mod command_line_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_query_returns_the_existing_raw_command_line() {
    let mut p = Process32::load(&command_line_executable::pe32(), 32).unwrap();
    let result = p.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (4, 2));
    let pointer = p.cpu.register(Register32::Eax);
    assert_eq!(p.cpu.register(Register32::Ebx), pointer);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 12];
    p.memory.read(u64::from(pointer), &mut bytes).unwrap();
    assert_eq!(&bytes, b"program.exe\0");
}

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, ProcessOptions};

const API: u32 = 0x7000_022c;
const STACK: u32 = 0x1000_ff00;

fn load(line: &[u8]) -> Process32 {
    Process32::load_with_options(
        &command_line_executable::pe32(),
        32,
        ProcessOptions {
            command_line: line,
            image_path: b"D:\\Other\\image.exe",
            current_directory: b"Q:\\Data",
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, stack: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, stack);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    p.memory
        .write(u64::from(stack), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.cpu
}

fn completed(p: &mut Process32, mut before: Cpu32, pointer: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    before.eip = 0x0040_1000;
    before.set_register(Register32::Eax, pointer);
    before.set_register(
        Register32::Esp,
        before.register(Register32::Esp).wrapping_add(4),
    );
    assert_eq!(p.cpu, before);
}

fn original_pointer(p: &Process32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(0x7000_200c, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn owned_raw_bytes_survive_input_release_and_crt_pointer_replacement() {
    for line in [
        &b""[..],
        b"  app\t\"two words\" ",
        b"\"other.exe\" \xff\x80",
    ] {
        let mut input = line.to_vec();
        let mut p = load(&input);
        let pointer = original_pointer(&p);
        input.fill(b'?');
        drop(input);
        p.memory
            .write(0x7000_200c, &0xffff_ffff_u32.to_le_bytes())
            .unwrap();
        p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
        for _ in 0..2 {
            let before = prepare(&mut p, STACK);
            completed(&mut p, before, pointer);
            let mut bytes = vec![0; line.len() + 1];
            p.memory.read(u64::from(pointer), &mut bytes).unwrap();
            assert_eq!(&bytes[..line.len()], line);
            assert_eq!(bytes[line.len()], 0);
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
    let mut a = load(b"first.exe");
    let mut b = load(b"other.exe");
    let pointer_a = original_pointer(&a);
    let pointer_b = original_pointer(&b);
    a.memory.write(u64::from(pointer_a), b"X").unwrap();
    for (p, pointer, expected) in [(&mut a, pointer_a, b'X'), (&mut b, pointer_b, b'o')] {
        let before = prepare(p, STACK);
        completed(p, before, pointer);
        let mut byte = [0];
        p.memory.read(u64::from(pointer), &mut byte).unwrap();
        assert_eq!(byte[0], expected);
    }
}

#[test]
fn query_only_reads_the_return_slot_even_at_the_end_of_guest32() {
    let mut p = load(b"app");
    let pointer = original_pointer(&p);
    for page in [0x7000_2000, 0x7000_4000, 0x7ffd_e000] {
        p.memory
            .protect(page, PAGE_SIZE, Permissions::NONE)
            .unwrap();
    }
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for stack in [STACK + 1, 0x1000_fffc, u32::MAX - 3] {
        let before = prepare(&mut p, stack);
        completed(&mut p, before, pointer);
    }
}

#[test]
fn frame_faults_and_zero_budget_preserve_cpu_and_memory() {
    let mut p = load(b"app");
    p.memory
        .map_zeroed(0xffff_f000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for stack in [0x1001_0000, u32::MAX - 2] {
        prepare(&mut p, STACK);
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.cpu.set_register(Register32::Esp, 0);
    let before = p.cpu;
    let result = p.run(0);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn authored_query_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = load(b"\"demo.exe\" --mode test");
        let pointer = original_pointer(&p);
        let (mut steps, mut calls) = (0, 0);
        loop {
            let result = p.run(budget);
            steps += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(steps + calls < 50);
        }
        assert_eq!((steps, calls), (4, 2));
        assert_eq!(p.cpu.register(Register32::Eax), pointer);
        assert_eq!(p.cpu.register(Register32::Ebx), pointer);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}
