#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01ac;
const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_2180;
const TEXT: u32 = 0x0040_2300;
const OUTPUT: u32 = 0x0040_2400;
const ERRNO: u32 = 0x7000_2020;

fn executable() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x68, 0xcd, 0xab, 0, 0, 0x6a, 0xd6, 0x68, 0x80, 0x21, 0x40, 0, 0x68, 0x00, 0x24, 0x40,
            0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 0x10, 0xcc,
        ],
        "mSvCrT.dll",
        &["sprintf"],
    );
    bytes[0x580..0x58a].copy_from_slice(b"%d:%X:%%\0\0");
    bytes
}

fn process() -> Process32 {
    Process32::load(&executable(), 128).unwrap()
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn bytes(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn prepare(p: &mut Process32, destination: u32, format: u32, values: &[u32]) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    words(p, STACK, &[0x0040_1000, destination, format]);
    words(p, STACK + 12, values);
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

fn failure(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let output = bytes(p, OUTPUT, 128);
    let result = p.run(1);
    if unsupported {
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    } else {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    assert_eq!(bytes(p, OUTPUT, 128), output);
}

#[test]
fn imported_sprintf_reads_variadic_words_and_uses_cdecl_cleanup() {
    for budget in [1, 40] {
        let mut p = process();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 40);
        }
        assert_eq!(counts, (7, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 10);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(bytes(&p, OUTPUT, 11), b"-42:ABCD:%\0");
    }
}

#[test]
fn supported_formats_share_the_crt_renderer_and_preserve_error_state() {
    let mut p = process();
    p.memory
        .write(u64::from(FORMAT), b"%d/%i/%u/%x/%X/%c/%s/%%\0")
        .unwrap();
    p.memory.write(u64::from(TEXT), b"\x80\xff\0").unwrap();
    words(&mut p, ERRNO, &[88]);
    words(&mut p, 0x7ffd_e034, &[77]);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    let before = prepare(
        &mut p,
        OUTPUT,
        FORMAT,
        &[
            0x8000_0000,
            0x7fff_ffff,
            u32::MAX,
            u32::MAX,
            0xabcd,
            0x1234_0141,
            TEXT,
        ],
    );
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    let expected = b"-2147483648/2147483647/4294967295/ffffffff/ABCD/A/\x80\xff/%\0";
    success(&mut p, before, u32::try_from(expected.len() - 1).unwrap());
    assert_eq!(bytes(&p, OUTPUT, expected.len()), expected);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    assert_eq!(bytes(&p, ERRNO, 4), 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn unused_variadic_words_are_not_read_but_consumed_words_must_exist() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(FORMAT), b"plain\0").unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    words(&mut p, 0xffff_fff4, &[0x0040_1000, OUTPUT, FORMAT]);
    let before = p.cpu;
    success(&mut p, before, 5);
    assert_eq!(bytes(&p, OUTPUT, 6), b"plain\0");

    p.memory.write(u64::from(FORMAT), b"%u\0").unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    let before = p.cpu;
    failure(&mut p, false);
    assert_eq!(p.cpu, before);
}

#[test]
fn invalid_inputs_and_output_faults_are_atomic_and_retryable() {
    let mut p = process();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 128]).unwrap();
    p.memory.write(u64::from(FORMAT), b"%f\0").unwrap();
    prepare(&mut p, OUTPUT, FORMAT, &[0]);
    failure(&mut p, true);

    p.memory.write(u64::from(FORMAT), b"changed\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, OUTPUT, FORMAT, &[]);
    failure(&mut p, false);
    assert_eq!(bytes(&p, OUTPUT, 8), b"!!!!!!!!");
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = p.cpu;
    success(&mut p, before, 7);
    assert_eq!(bytes(&p, OUTPUT, 8), b"changed\0");

    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0, FORMAT, &[]);
    failure(&mut p, false);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = p.cpu;
    success(&mut p, before, u32::MAX);
    assert_eq!(bytes(&p, ERRNO, 4), 22_u32.to_le_bytes());

    words(&mut p, ERRNO, &[88]);
    let before = prepare(&mut p, OUTPUT, 0, &[]);
    success(&mut p, before, u32::MAX);
    assert_eq!(bytes(&p, ERRNO, 4), 22_u32.to_le_bytes());
}

#[test]
fn snapshotted_aliases_and_return_address_writes_remain_observable() {
    let mut p = process();
    p.memory.write(u64::from(FORMAT), b"%s:%x\0").unwrap();
    p.memory.write(u64::from(TEXT), b"test\0").unwrap();
    let before = prepare(&mut p, FORMAT, FORMAT, &[TEXT, 0xab]);
    success(&mut p, before, 7);
    assert_eq!(bytes(&p, FORMAT, 8), b"test:ab\0");

    p.memory.write(u64::from(FORMAT), b"ABCD\0").unwrap();
    let mut expected = prepare(&mut p, STACK, FORMAT, &[]);
    let result = p.run(1);
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x4443_4241;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, 4);
    assert_eq!(p.cpu, expected);
}
