#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/registry_default_executable.rs"]
mod registry_default_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_default_value_is_created_and_read_through_an_ordinary_key_handle() {
    for budget in [1, 100] {
        let mut p = Process32::load(&registry_default_executable::pe32(), 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (20, 3));
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 6];
        p.memory.read(0x0040_2220, &mut bytes).unwrap();
        assert_eq!(&bytes, b"hello\0");
    }
}

const SET: u32 = 0x7000_0298;
const CU: u32 = 0x8000_0001;
const LM: u32 = 0x8000_0002;
const CR: u32 = 0x8000_0000;
const PATH: u32 = 0x0040_2400;
const DATA: u32 = 0x0040_2600;
const OUT: u32 = 0x0040_2800;
const HANDLE: u32 = 0x0040_27f0;
const TYPE: u32 = HANDLE + 4;
const SIZE: u32 = HANDLE + 8;
const STACK: u32 = 0x1000_ff00;

fn load() -> Process32 {
    Process32::load(&registry_default_executable::pe32(), 128).unwrap()
}
fn put(p: &mut Process32, pointer: u32, value: u32) {
    p.memory
        .write(u64::from(pointer), &value.to_le_bytes())
        .unwrap();
}
fn word(p: &Process32, pointer: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(pointer), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn path(p: &mut Process32, bytes: &[u8]) {
    p.memory.write(u64::from(PATH), bytes).unwrap();
    p.memory
        .write(u64::from(PATH) + bytes.len() as u64, &[0])
        .unwrap();
}
fn prepare(p: &mut Process32, api: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    for (i, value) in std::iter::once(0x0040_1000)
        .chain(args.iter().copied())
        .enumerate()
    {
        put(p, STACK + u32::try_from(i).unwrap() * 4, value);
    }
}
fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let mut cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = p.cpu.register(Register32::Eax);
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, result);
    cpu.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
    result
}
fn stopped(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let run = p.run(1);
    if unsupported {
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: cpu.eip });
    } else {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
}
fn set(p: &mut Process32, root: u32, key: &[u8], value: &[u8], declared: u32) {
    path(p, key);
    p.memory.write(u64::from(DATA), value).unwrap();
    assert_eq!(call(p, SET, &[root, PATH, 1, DATA, declared]), 0);
}
fn open(p: &mut Process32, root: u32, key: &[u8], access: u32) -> u32 {
    path(p, key);
    assert_eq!(call(p, 0x7000_0278, &[root, PATH, 0, access, HANDLE]), 0);
    word(p, HANDLE)
}
fn expect_value(p: &mut Process32, handle: u32, expected: &[u8]) {
    put(p, SIZE, 512);
    assert_eq!(call(p, 0x7000_0284, &[handle, 0, 0, TYPE, OUT, SIZE]), 0);
    assert_eq!(word(p, TYPE), 1);
    assert_eq!(word(p, SIZE), u32::try_from(expected.len()).unwrap());
    let mut bytes = vec![0; expected.len()];
    p.memory.read(u64::from(OUT), &mut bytes).unwrap();
    assert_eq!(bytes, expected);
}

#[test]
fn ignored_size_empty_subkeys_and_handle_replacement_share_value_storage() {
    let mut p = load();
    set(
        &mut p,
        CU,
        b"Software\\Example\\Child",
        b"Mixed Case\0trailing",
        0,
    );
    let handle = open(&mut p, CU, b"software\\example\\child", 3);
    assert_eq!(handle, 0x7600_0004);
    expect_value(&mut p, handle, b"Mixed Case\0");
    set(&mut p, handle, b"", b"replacement\0", u32::MAX);
    expect_value(&mut p, handle, b"replacement\0");
    p.memory.write(u64::from(DATA), b"\0").unwrap();
    assert_eq!(call(&mut p, SET, &[handle, 0, 1, DATA, 500]), 0);
    expect_value(&mut p, handle, b"\0");
    let read_only = open(&mut p, CU, b"software\\example\\child", 1);
    assert_eq!(
        call(&mut p, SET, &[read_only, u32::MAX, 99, u32::MAX, 0]),
        5
    );
    assert_eq!(call(&mut p, SET, &[123, u32::MAX, 99, u32::MAX, 0]), 6);
    assert_eq!(call(&mut p, 0x7000_0280, &[handle]), 0);
    assert_eq!(call(&mut p, SET, &[handle, 0, 1, DATA, 0]), 6);
    expect_value(&mut p, read_only, b"\0");
}

#[test]
fn classes_writes_target_existing_user_keys_and_create_new_machine_keys() {
    let mut p = load();
    set(&mut p, CR, b"Demo\\Child", b"machine\0", 1);
    let machine = open(&mut p, LM, b"software\\classes\\Demo\\Child", 1);
    expect_value(&mut p, machine, b"machine\0");
    path(&mut p, b"software\\classes\\Demo");
    assert_eq!(
        call(&mut p, 0x7000_027c, &[CU, PATH, 0, 0, 0, 3, 0, HANDLE, 0]),
        0
    );
    set(&mut p, CR, b"Demo\\New", b"new machine\0", 1);
    let new_machine = open(&mut p, LM, b"software\\classes\\Demo\\New", 1);
    expect_value(&mut p, new_machine, b"new machine\0");
    path(&mut p, b"software\\classes\\Demo\\New");
    assert_eq!(call(&mut p, 0x7000_0278, &[CU, PATH, 0, 1, HANDLE]), 2);
    set(
        &mut p,
        CU,
        b"software\\classes\\demo\\child",
        b"old user\0",
        0,
    );
    set(&mut p, CR, b"DEMO\\CHILD", b"user\0", u32::MAX);
    let user = open(&mut p, CU, b"software\\classes\\demo\\child", 1);
    expect_value(&mut p, user, b"user\0");
    expect_value(&mut p, machine, b"machine\0");
    prepare(&mut p, 0x7000_0278, &[CR, PATH, 0, 1, HANDLE]);
    stopped(&mut p, true);
}

#[test]
fn malformed_or_unreadable_values_never_leave_new_keys_or_replace_old_values() {
    let mut p = load();
    for (data, unsupported) in [(b"\x80\0".as_slice(), true), (b"x", false)] {
        let source = if unsupported { DATA } else { 0x0040_2fff };
        path(&mut p, b"software\\missing\\child");
        p.memory.write(u64::from(source), data).unwrap();
        prepare(&mut p, SET, &[CU, PATH, 1, source, 0]);
        stopped(&mut p, unsupported);
        assert_eq!(call(&mut p, 0x7000_0278, &[CU, PATH, 0, 1, HANDLE]), 2);
    }
    set(&mut p, CU, b"software\\stable", b"keep\0", 0);
    let stable = open(&mut p, CU, b"software\\stable", 3);
    prepare(&mut p, SET, &[stable, 0, 1, 0x6000_0000, 0]);
    stopped(&mut p, false);
    expect_value(&mut p, stable, b"keep\0");
    for (key, kind, data) in [
        (b"bad\\\\key".as_slice(), 1, DATA),
        (b"ok", 3, DATA),
        (b"ok", 1, 0),
    ] {
        path(&mut p, key);
        prepare(&mut p, SET, &[CU, PATH, kind, data, 0]);
        stopped(&mut p, true);
    }
    path(&mut p, b"forbidden");
    assert_eq!(call(&mut p, SET, &[LM, PATH, 1, DATA, 0]), 5);
}

#[test]
fn bounded_data_and_32bit_edges_are_scanned_without_truncation() {
    let mut p = load();
    p.memory
        .map_zeroed(0x2000_0000, 65536, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'x'; 65536]).unwrap();
    path(&mut p, b"software\\boundary");
    prepare(&mut p, SET, &[CU, PATH, 1, 0x2000_0000, 1]);
    stopped(&mut p, true);
    assert_eq!(call(&mut p, 0x7000_0278, &[CU, PATH, 0, 1, HANDLE]), 2);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    assert_eq!(call(&mut p, SET, &[CU, PATH, 1, 0x2000_0000, 0]), 0);
    let handle = open(&mut p, CU, b"software\\boundary", 3);
    assert_eq!(call(&mut p, 0x7000_0284, &[handle, 0, 0, TYPE, 0, SIZE]), 0);
    assert_eq!(word(&p, SIZE), 65536);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, SET, &[handle, 0, 1, u32::MAX, 0]), 0);
    expect_value(&mut p, handle, b"\0");
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    prepare(&mut p, SET, &[handle, 0, 1, u32::MAX, 0]);
    stopped(&mut p, false);
    expect_value(&mut p, handle, b"\0");
}

#[test]
fn writes_ignore_error_cells_and_zero_budget_or_incomplete_frames_do_not_create_keys() {
    let mut p = load();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    set(&mut p, CU, b"software\\value", b"text\0", 0);
    path(&mut p, b"software\\later");
    prepare(&mut p, SET, &[CU, PATH, 1, DATA, 0]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_ffec);
    stopped(&mut p, false);
    assert_eq!(call(&mut p, 0x7000_0278, &[CU, PATH, 0, 1, HANDLE]), 2);
}
