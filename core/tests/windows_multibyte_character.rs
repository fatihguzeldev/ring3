#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_033c;
const STACK: u32 = 0x1000_ef00;
const SOURCE: u32 = 0x0040_2300;
const OUTPUT: u32 = 0x0040_2400;

fn process(source: &[u8]) -> Process32 {
    let mut process = Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["MultiByteToWideChar"]),
        32,
    )
    .unwrap();
    process.memory.write(u64::from(SOURCE), source).unwrap();
    process
        .memory
        .write(u64::from(OUTPUT), &[0x55; 16])
        .unwrap();
    process
}

fn prepare(process: &mut Process32, args: [u32; 6]) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(0x0040_1000_u32)
        .chain(args)
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
}

fn output(process: &Process32) -> [u8; 16] {
    let mut bytes = [0; 16];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

#[test]
fn null_terminated_cp1252_conversion_includes_terminator_and_cleans_stack() {
    let mut process = process(b"A\x80\xff\0");
    prepare(&mut process, [0, 0, SOURCE, u32::MAX, OUTPUT, 8]);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 4);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 28);
    assert_eq!(
        &output(&process)[..8],
        &[b'A', 0, 0xac, 0x20, 0xff, 0, 0, 0]
    );
}

#[test]
fn explicit_count_and_size_query_do_not_invent_terminator() {
    let mut process = process(b"AB\0C");
    prepare(&mut process, [3, 0, SOURCE, 2, 0, 0]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 2);
    assert_eq!(output(&process), [0x55; 16]);
    prepare(&mut process, [1252, 0, SOURCE, 2, OUTPUT, 2]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 2);
    assert_eq!(&output(&process)[..4], &[b'A', 0, b'B', 0]);
}

#[test]
fn capacity_and_invalid_arguments_preserve_output() {
    let mut process = process(b"ABC\0");
    prepare(&mut process, [0, 0, SOURCE, u32::MAX, OUTPUT, 3]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 122);
    assert_eq!(output(&process), [0x55; 16]);
    for args in [
        [0, 0, 0, u32::MAX, OUTPUT, 4],
        [0, 0, SOURCE, 0, OUTPUT, 4],
        [0, 0, SOURCE, (-2_i32).cast_unsigned(), OUTPUT, 4],
        [0, 0, SOURCE, u32::MAX, 0, 4],
        [0, 0, SOURCE, u32::MAX, OUTPUT, (-1_i32).cast_unsigned()],
    ] {
        prepare(&mut process, args);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.last_error().unwrap(), 87);
        assert_eq!(output(&process), [0x55; 16]);
    }
}

#[test]
fn unsupported_code_page_flags_and_unmapped_output_do_not_write() {
    for (source, args, memory_fault) in [
        (
            b"A\0".as_slice(),
            [65001, 0, SOURCE, u32::MAX, OUTPUT, 2],
            false,
        ),
        (b"A\0", [0, 1, SOURCE, u32::MAX, OUTPUT, 2], false),
        (b"\x81\0", [0, 0, SOURCE, u32::MAX, OUTPUT, 2], false),
        (b"A\0", [0, 0, SOURCE, u32::MAX, 0xffff_0000, 2], true),
    ] {
        let mut process = process(source);
        prepare(&mut process, args);
        let before = process.cpu;
        let reason = process.run(1).reason;
        if memory_fault {
            assert!(matches!(
                reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        } else {
            assert_eq!(reason, ProcessStop::UnsupportedApi { address: API });
        }
        assert_eq!(process.cpu, before);
        assert_eq!(output(&process), [0x55; 16]);
    }
}
