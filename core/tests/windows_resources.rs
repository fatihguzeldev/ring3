#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/resource_executable.rs"]
mod resource_executable;

use ring3_core::execution::{
    GuestModule, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const FIND: u32 = 0x7000_00e8;
const STRING: u32 = 0x7000_00ec;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2180;

fn load() -> Process32 {
    Process32::load(&resource_executable::guest(), 32).unwrap()
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(process, api, args);
    let mut expected = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from((args.len() + 1) * 4).unwrap(),
    );
    assert_eq!(process.cpu, expected);
    value
}

fn output(process: &Process32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

#[test]
fn numeric_resource_identity_and_counted_string_slots_come_from_the_image() {
    let mut process = load();
    let pages = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    for module in [0, 0x0040_0000] {
        assert_eq!(call(&mut process, FIND, &[module, 1, 6]), 0x0040_2348);
    }
    for (id, capacity, expected) in [
        (0, 6, b"Alpha\0".as_slice()),
        (0x10000, 4, b"Alp\0"),
        (1, 99, b"Beta\0"),
        (15, 1, b"\0"),
    ] {
        process
            .memory
            .write(u64::from(OUTPUT), &[0x55; 10])
            .unwrap();
        assert_eq!(
            call(&mut process, STRING, &[0, id, OUTPUT, capacity]),
            u32::try_from(expected.len() - 1).unwrap()
        );
        assert_eq!(output(&process, expected.len()), expected);
        assert_eq!(output(&process, expected.len() + 1)[expected.len()], 0x55);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn missing_resources_report_the_lookup_stage_and_clear_string_output() {
    let mut process = load();
    for (args, error) in [
        ([0, 1, 10], 1813),
        ([0, 2, 6], 1814),
        ([0x7000_0800, 1, 6], 1812),
        ([123, 1, 6], 87),
    ] {
        assert_eq!(call(&mut process, FIND, &args), 0);
        assert_eq!(process.last_error().unwrap(), error);
    }
    process.memory.write(u64::from(OUTPUT), b"!!").unwrap();
    assert_eq!(call(&mut process, STRING, &[0, 16, OUTPUT, 10]), 0);
    assert_eq!(process.last_error().unwrap(), 1814);
    assert_eq!(output(&process, 2), [0, b'!']);
    let empty = resource_executable::pe32(&[0xcc], &[]);
    let mut process = Process32::load(&empty, 32).unwrap();
    assert_eq!(call(&mut process, FIND, &[0, 1, 6]), 0);
    assert_eq!(process.last_error().unwrap(), 1815);
}

#[test]
fn provided_images_override_builtin_identity_and_own_their_metadata() {
    check_provider_resources(0x5000_0000, 0x5000_0000);
    check_provider_resources(0x1000_0000, 0x3000_0000);
}

fn check_provider_resources(preferred: u32, actual: u32) {
    let exe = resource_executable::guest();
    let text = resource_executable::block(&[&[68, 76, 76]]);
    let mut dll = resource_executable::pe32(&[0xcc], &[(1033, &text)]);
    resource_executable::put(&mut dll, 0xb4, preferred);
    resource_executable::put(&mut dll, 0xa8, 0);
    resource_executable::put(&mut dll, 0x100, 0);
    resource_executable::put(&mut dll, 0x104, 0);
    for (at, value) in [(0x120, 0x2f00), (0x124, 12), (0x1300, 0), (0x1304, 12)] {
        resource_executable::put(&mut dll, at, value);
    }
    dll[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    let mut process = Process32::load_with_options(
        &exe,
        40,
        ProcessOptions {
            modules: &[GuestModule {
                name: "gdi32.dll",
                bytes: &dll,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    dll.fill(0);
    process
        .memory
        .write(u64::from(OUTPUT), b"gdi32.dll\0")
        .unwrap();
    assert_eq!(call(&mut process, 0x7000_0018, &[OUTPUT]), actual);
    assert_eq!(call(&mut process, FIND, &[actual, 1, 6]), actual + 0x2348);
    assert_eq!(call(&mut process, STRING, &[actual, 0, OUTPUT, 10]), 3);
    assert_eq!(output(&process, 4), b"DLL\0");
    assert_eq!(call(&mut process, FIND, &[0x7000_0810, 1, 6]), 0);
    assert_eq!(process.last_error().unwrap(), 87);
    process
        .memory
        .write(u64::from(actual + 0x2310), &10_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, FIND, &[actual, 1, 6]), actual + 0x2348);
    process
        .memory
        .write(u64::from(actual + 0x235a), &88_u16.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, STRING, &[actual, 0, OUTPUT, 10]), 3);
    assert_eq!(output(&process, 4), b"XLL\0");
}

#[test]
fn neutral_us_primary_and_first_language_fallbacks_are_deterministic() {
    let a = resource_executable::block(&[&[65]]);
    let b = resource_executable::block(&[&[66]]);
    for (languages, expected) in [
        (vec![(0, a.as_slice()), (1033, b.as_slice())], b'A'),
        (vec![(9, a.as_slice()), (1033, b.as_slice())], b'B'),
        (vec![(9, a.as_slice()), (1041, b.as_slice())], b'A'),
        (vec![(1031, a.as_slice()), (1041, b.as_slice())], b'A'),
    ] {
        let mut process =
            Process32::load(&resource_executable::pe32(&[0xcc], &languages), 32).unwrap();
        assert_eq!(call(&mut process, STRING, &[0, 0, OUTPUT, 2]), 1);
        assert_eq!(output(&process, 2), [expected, 0]);
    }
}

#[test]
fn unsupported_metadata_and_text_do_not_mutate_guest_state() {
    let mut bytes = resource_executable::guest();
    resource_executable::put(&mut bytes, 0x748, u32::MAX);
    resource_executable::put(&mut bytes, 0x74c, 0);
    let mut empty = Process32::load(&bytes, 32).unwrap();
    assert_eq!(call(&mut empty, FIND, &[0, 1, 6]), 0x0040_2348);
    prepare(&mut empty, STRING, &[0, 0, OUTPUT, 1]);
    assert_eq!(
        empty.run(1).reason,
        ProcessStop::UnsupportedApi { address: STRING }
    );
    for (offset, value) in [(0x10c, 1), (0x714, 0), (0x740, 0x10000)] {
        let mut bytes = resource_executable::guest();
        resource_executable::put(&mut bytes, offset, value);
        let mut process = Process32::load(&bytes, 32).unwrap();
        prepare(&mut process, FIND, &[0, 1, 6]);
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: FIND });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    let content = resource_executable::block(&[&[65, 0, 66, 0x100]]);
    let mut process =
        Process32::load(&resource_executable::pe32(&[0xcc], &[(1033, &content)]), 32).unwrap();
    assert_eq!(call(&mut process, STRING, &[0, 0, OUTPUT, 4]), 3);
    assert_eq!(output(&process, 4), [65, 0, 66, 0]);
    for (api, args) in [
        (STRING, vec![0, 0, OUTPUT, 5]),
        (STRING, vec![0, 0, OUTPUT, 0]),
        (STRING, vec![0, 0, OUTPUT, u32::MAX]),
        (FIND, vec![0, OUTPUT, 6]),
    ] {
        prepare(&mut process, api, &args);
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(output(&process, 4), [65, 0, 66, 0]);
    }
    process
        .memory
        .write(0x0040_2358, &u16::MAX.to_le_bytes())
        .unwrap();
    prepare(&mut process, STRING, &[0, 0, OUTPUT, 1]);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: STRING }
    );
    assert_eq!(process.cpu, before);
}

#[test]
fn source_output_and_error_write_faults_preserve_cpu_and_copy_bytes() {
    for (output, missing, protected_error) in [
        (0x0040_2ffe, false, false),
        (u32::MAX, false, false),
        (OUTPUT, true, true),
    ] {
        let mut process = load();
        process.memory.write(u64::from(OUTPUT), b"!!!").unwrap();
        if protected_error {
            process
                .memory
                .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        prepare(
            &mut process,
            STRING,
            &[0, if missing { 16 } else { 0 }, output, 10],
        );
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        let mut bytes = [0; 3];
        process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(&bytes, b"!!!");
    }
    let mut process = load();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, FIND, &[0, 1, 6]), 0x0040_2348);
    prepare(&mut process, STRING, &[0, 0, STACK + 24, 5]);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}

#[test]
fn output_aliases_use_snapshots_and_last_error_wins_on_missing_strings() {
    let mut process = load();
    assert_eq!(call(&mut process, STRING, &[0, 16, 0x7ffd_e034, 1]), 0);
    assert_eq!(process.last_error().unwrap(), 1814);
    assert_eq!(call(&mut process, STRING, &[0, 0, 0x0040_235a, 4]), 3);
    let mut bytes = [0; 4];
    process.memory.read(0x0040_235a, &mut bytes).unwrap();
    assert_eq!(&bytes, b"Alp\0");
    let mut process = load();
    prepare(&mut process, STRING, &[0, 0, STACK, 4]);
    let result = process.run(1);
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.eip, 0x0070_6c41);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 20);
    prepare(&mut process, STRING, &[0, 0, OUTPUT, 6]);
    process.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}
