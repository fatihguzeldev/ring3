use super::registry_value_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_values_run_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = Process32::load(&registry_value_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 30);
        }
        assert_eq!(counts, (16, 2));
        assert_eq!(p.cpu.register(Register32::Eax), 0x7856_3412);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut words = [0; 8];
        p.memory.read(0x0040_2180, &mut words).unwrap();
        assert_eq!(words, [4, 0, 0, 0, 4, 0, 0, 0]);
    }
}

const ROOT: u32 = 0x8000_0001;
const SET: u32 = 0x7000_0288;
const QUERY: u32 = 0x7000_0284;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2400;
const DATA: u32 = 0x0040_2600;
const OUT: u32 = 0x0040_2800;
const TYPE: u32 = OUT + 16;
const SIZE: u32 = OUT + 20;

fn load() -> Process32 {
    Process32::load(&registry_value_executable::pe32(), 128).unwrap()
}

fn put(p: &mut Process32, pointer: u32, value: u32) {
    p.memory
        .write(u64::from(pointer), &value.to_le_bytes())
        .unwrap();
}

fn word(p: &Process32, pointer: u32) -> u32 {
    let mut data = [0; 4];
    p.memory.read(u64::from(pointer), &mut data).unwrap();
    u32::from_le_bytes(data)
}

fn name(p: &mut Process32, value: &[u8]) {
    p.memory.write(u64::from(NAME), value).unwrap();
    p.memory
        .write(u64::from(NAME) + value.len() as u64, &[0])
        .unwrap();
}

fn prepare(p: &mut Process32, address: u32, args: &[u32]) -> ring3_core::execution::Cpu32 {
    p.cpu.eip = address;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    put(p, STACK, 0x0040_1000);
    for (index, &arg) in args.iter().enumerate() {
        put(p, STACK + 4 + u32::try_from(index).unwrap() * 4, arg);
    }
    p.cpu
}

fn call(p: &mut Process32, address: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, address, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = word(p, STACK);
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, expected);
    value
}

fn set(p: &mut Process32, handle: u32, kind: u32, bytes: &[u8]) -> u32 {
    p.memory.write(u64::from(DATA), bytes).unwrap();
    call(
        p,
        SET,
        &[
            handle,
            NAME,
            0,
            kind,
            DATA,
            u32::try_from(bytes.len()).unwrap(),
        ],
    )
}

fn query(p: &mut Process32, handle: u32, capacity: u32) -> u32 {
    put(p, SIZE, capacity);
    call(p, QUERY, &[handle, NAME, 0, TYPE, OUT, SIZE])
}

#[test]
fn typed_values_preserve_bytes_case_and_unnamed_identity() {
    let mut p = load();
    for (kind, bytes) in [
        (0, b"\xff\0".as_slice()),
        (1, b"Mixed Case\0"),
        (2, b"%TEMP%\0"),
        (3, b"\xff\x80\0"),
        (4, b"\x12\x34\x56\x78"),
        (5, b"\x78\x56\x34\x12"),
        (7, b"One\0Two\0\0"),
        (7, b"\0"),
        (11, b"\x01\0\0\0\0\0\0\x80"),
    ] {
        name(&mut p, b"a\\Setting");
        assert_eq!(set(&mut p, ROOT, kind, bytes), 0);
        name(&mut p, b"A\\SETTING");
        p.memory.write(u64::from(OUT), &[0x55; 16]).unwrap();
        assert_eq!(query(&mut p, ROOT, 16), 0);
        assert_eq!(word(&p, TYPE), kind);
        assert_eq!(word(&p, SIZE), u32::try_from(bytes.len()).unwrap());
        let mut read = [0; 16];
        p.memory.read(u64::from(OUT), &mut read).unwrap();
        assert_eq!(&read[..bytes.len()], bytes);
        assert!(read[bytes.len()..].iter().all(|&byte| byte == 0x55));
    }
    name(&mut p, b"");
    assert_eq!(query(&mut p, ROOT, 16), 2);
    assert_eq!(set(&mut p, ROOT, 3, b""), 0);
    assert_eq!(call(&mut p, QUERY, &[ROOT, 0, 0, TYPE, u32::MAX, SIZE]), 0);
    assert_eq!(word(&p, SIZE), 0);
    assert_eq!(call(&mut p, SET, &[ROOT, 0, 0, 0, 0, 0]), 0);
    assert_eq!(query(&mut p, ROOT, 0), 0);
    assert_eq!(word(&p, TYPE), 0);
}

#[test]
fn values_survive_handle_closure_and_access_is_checked_before_pointers() {
    let mut p = load();
    name(&mut p, b"Software\\Example");
    assert_eq!(
        call(&mut p, 0x7000_027c, &[ROOT, NAME, 0, 0, 0, 3, 0, OUT, 0]),
        0
    );
    let key = word(&p, OUT);
    name(&mut p, b"Setting");
    assert_eq!(set(&mut p, key, 4, &42_u32.to_le_bytes()), 0);
    assert_eq!(call(&mut p, 0x7000_0280, &[key]), 0);
    assert_eq!(query(&mut p, key, 4), 6);
    name(&mut p, b"SOFTWARE\\EXAMPLE");
    assert_eq!(call(&mut p, 0x7000_0278, &[ROOT, NAME, 0, 1, OUT]), 0);
    let read = word(&p, OUT);
    assert_eq!(call(&mut p, 0x7000_0278, &[ROOT, NAME, 0, 2, OUT]), 0);
    let write = word(&p, OUT);
    name(&mut p, b"setting");
    assert_eq!(query(&mut p, read, 4), 0);
    assert_eq!(word(&p, OUT), 42);
    assert_eq!(call(&mut p, SET, &[read, u32::MAX, 0, 3, u32::MAX, 4]), 5);
    assert_eq!(call(&mut p, QUERY, &[write, u32::MAX, 0, 0, 0, 0]), 5);
    assert_eq!(set(&mut p, write, 4, &43_u32.to_le_bytes()), 0);
    assert_eq!(query(&mut p, read, 4), 0);
    assert_eq!(word(&p, OUT), 43);
    let mut other = load();
    name(&mut other, b"setting");
    assert_eq!(query(&mut other, ROOT, 4), 2);
    assert_eq!(query(&mut other, read, 4), 6);
}

#[test]
fn optional_outputs_size_only_and_short_buffers_report_actual_size() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    name(&mut p, b"value");
    assert_eq!(set(&mut p, ROOT, 3, b"abcd"), 0);
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, 0, 0, 0]), 0);
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, TYPE, 0, 0]), 0);
    assert_eq!(word(&p, TYPE), 3);
    p.memory
        .map_zeroed(
            0x3000_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, 0, 0, 0x3000_0000]), 0);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(word(&p, 0x3000_0000), 4);
    for capacity in [0, 3] {
        put(&mut p, OUT, 0x1234_5678);
        assert_eq!(query(&mut p, ROOT, capacity), 234);
        assert_eq!(word(&p, OUT), 0x1234_5678);
        assert_eq!(word(&p, TYPE), 3);
        assert_eq!(word(&p, SIZE), 4);
        put(&mut p, SIZE, capacity);
        assert_eq!(
            call(&mut p, QUERY, &[ROOT, NAME, 0, TYPE, u32::MAX, SIZE]),
            234
        );
    }
    name(&mut p, b"missing");
    assert_eq!(
        call(
            &mut p,
            QUERY,
            &[ROOT, NAME, 0, u32::MAX, u32::MAX, u32::MAX]
        ),
        2
    );
}

#[test]
fn unsupported_shapes_and_malformed_data_preserve_existing_values() {
    let mut p = load();
    name(&mut p, b"value");
    assert_eq!(set(&mut p, ROOT, 3, b"abcd"), 0);
    for (kind, data) in [
        (1, b"no nul".as_slice()),
        (1, b"\xff\0"),
        (2, b""),
        (4, b"abc"),
        (5, b"abcde"),
        (7, b"a\0"),
        (11, b"short"),
        (6, b"link\0"),
        (0xffff_ffff, b""),
    ] {
        p.memory.write(u64::from(DATA), data).unwrap();
        let args = [
            ROOT,
            NAME,
            0,
            kind,
            DATA,
            u32::try_from(data.len()).unwrap(),
        ];
        let before = prepare(&mut p, SET, &args);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(query(&mut p, ROOT, 4), 0);
        assert_eq!(word(&p, OUT), u32::from_le_bytes(*b"abcd"));
    }
    for (address, args) in [
        (SET, [ROOT, NAME, 1, 3, DATA, 0]),
        (QUERY, [ROOT, NAME, 1, 0, 0, 0]),
        (QUERY, [ROOT, NAME, 0, 0, OUT, 0]),
        (QUERY, [0x8000_0050, NAME, 0, 0, 0, 0]),
    ] {
        let before = prepare(&mut p, address, &args);
        assert_eq!(p.run(1).reason, ProcessStop::UnsupportedApi { address });
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn source_and_output_faults_are_atomic_and_budgets_preserve_state() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    name(&mut p, b"value");
    assert_eq!(set(&mut p, ROOT, 3, b"abcd"), 0);
    for pointer in [0xffff_fffe, 0x0040_2ffe] {
        let before = prepare(&mut p, SET, &[ROOT, NAME, 0, 3, pointer, 4]);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(run.api_calls, 0);
        assert_eq!(p.cpu, before);
    }
    for (kind, data) in [(TYPE, 0xffff_fffe), (u32::MAX, OUT), (TYPE, 0x0040_2ffe)] {
        put(&mut p, TYPE, 123);
        put(&mut p, SIZE, 4);
        put(&mut p, OUT, 456);
        let before = prepare(&mut p, QUERY, &[ROOT, NAME, 0, kind, data, SIZE]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, TYPE), 123);
        assert_eq!(word(&p, SIZE), 4);
        assert_eq!(word(&p, OUT), 456);
    }
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x3000_0000, 4);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, QUERY, &[ROOT, NAME, 0, TYPE, OUT, 0x3000_0000]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(word(&p, TYPE), 123);
    assert_eq!(word(&p, OUT), 456);
    let before = prepare(&mut p, SET, &[ROOT, NAME, 0, 3, DATA, 0]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(query(&mut p, ROOT, 4), 0);
    assert_eq!(word(&p, OUT), u32::from_le_bytes(*b"abcd"));
}

#[test]
fn names_are_bounded_and_zero_length_data_does_not_dereference() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    p.memory
        .map_zeroed(0, 4096, Permissions::READ_WRITE)
        .unwrap();
    name(&mut p, b"value");
    let before = prepare(&mut p, SET, &[ROOT, NAME, 0, 3, 0, 1]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: SET }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(query(&mut p, ROOT, 4), 2);
    for value in [&vec![b'a'; 256], b"\xff".as_slice()] {
        name(&mut p, value);
        let before = prepare(&mut p, SET, &[ROOT, NAME, 0, 3, DATA, 0]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.cpu, before);
    }
    name(&mut p, &vec![b'a'; 255]);
    assert_eq!(call(&mut p, SET, &[ROOT, NAME, 0, 3, u32::MAX, 0]), 0);
    assert_eq!(query(&mut p, ROOT, 0), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_ffff, b"v").unwrap();
    let before = prepare(&mut p, QUERY, &[ROOT, u32::MAX, 0, 0, 0, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory.write(0xffff_fffe, b"v\0").unwrap();
    assert_eq!(call(&mut p, SET, &[ROOT, 0xffff_fffe, 0, 0, 0, 0]), 0);
    assert_eq!(
        call(&mut p, QUERY, &[ROOT, 0xffff_fffe, 0, TYPE, 0, SIZE]),
        0
    );
    assert_eq!(word(&p, SIZE), 0);
    assert_eq!(call(&mut p, SET, &[ROOT, NAME, 0, 3, u32::MAX, 65537]), 8);
}

#[test]
fn output_aliases_snapshot_names_capacities_and_reread_returns() {
    let mut p = load();
    name(&mut p, b"value");
    assert_eq!(set(&mut p, ROOT, 3, b"abcd"), 0);
    put(&mut p, SIZE, 4);
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, TYPE, NAME, SIZE]), 0);
    assert_eq!(word(&p, NAME), u32::from_le_bytes(*b"abcd"));
    name(&mut p, b"value");
    put(&mut p, SIZE, 4);
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, SIZE, SIZE, SIZE]), 0);
    assert_eq!(word(&p, SIZE), 4);
    put(&mut p, SIZE, 4);
    assert_eq!(call(&mut p, QUERY, &[ROOT, NAME, 0, TYPE, STACK, SIZE]), 0);
    assert_eq!(p.cpu.eip, u32::from_le_bytes(*b"abcd"));
}

#[test]
fn status_paths_preserve_error_cells_without_accessing_them() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7000_2020, 88);
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    name(&mut p, b"value");
    assert_eq!(query(&mut p, ROOT, 4), 2);
    assert_eq!(set(&mut p, ROOT, 3, b"abcd"), 0);
    assert_eq!(query(&mut p, ROOT, 3), 234);
    assert_eq!(query(&mut p, ROOT, 4), 0);
    for address in [SET, QUERY] {
        assert_eq!(call(&mut p, address, &[1, u32::MAX, 0, 0, 0, 0]), 6);
    }
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory
            .protect(page, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7000_2020), 88);
}
