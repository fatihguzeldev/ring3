use super::imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const ALLOC: u32 = 0x7000_0068;
const FREE: u32 = 0x7000_006c;
const GET: u32 = 0x7000_0070;
const SET: u32 = 0x7000_0074;
const STACK: u32 = 0x1000_ff00;
const CREATE: u32 = 0x7000_0548;
const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;

fn process() -> Process32 {
    process_with_pages(32)
}

fn process_with_pages(pages: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "kernel32.dll",
            &["TlsAlloc", "TlsFree", "TlsGetValue", "TlsSetValue"],
        ),
        pages,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32, arguments: &[u32]) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    for (index, value) in std::iter::once(&0x0040_1000_u32)
        .chain(arguments)
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(p: &mut Process32, api: u32, arguments: &[u32]) -> u32 {
    prepare(p, api, arguments);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x0040_1000);
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK
            + if api == CREATE {
                4
            } else {
                (u32::try_from(arguments.len()).unwrap() + 1) * 4
            }
    );
    p.cpu.register(Register32::Eax)
}

fn child(p: &mut Process32) -> u32 {
    call(p, CREATE, &[0, 0, 0x0040_1000, 0, 4, 0])
}

#[test]
fn registered_threads_keep_values_and_last_error_separate() {
    let mut p = process_with_pages(80);
    assert_eq!(call(&mut p, ALLOC, &[]), 0);
    call(&mut p, SET, &[0, 111]);
    p.memory
        .write(u64::from(PRIMARY + 0x34), &77_u32.to_le_bytes())
        .unwrap();
    child(&mut p);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, GET, &[0]), 0);
    call(&mut p, SET, &[0, 222]);
    assert_eq!(call(&mut p, GET, &[0]), 222);
    assert_eq!(p.last_error().unwrap(), 0);
    call(&mut p, GET, &[64]);
    assert_eq!(p.last_error().unwrap(), 87);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(call(&mut p, GET, &[0]), 111);
    child(&mut p);
    p.cpu.set_fs_base(CHILD + 0x11000);
    assert_eq!(call(&mut p, GET, &[0]), 0);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(p.last_error().unwrap(), 87);
    assert_eq!(call(&mut p, GET, &[0]), 222);
}

#[test]
fn allocation_and_reuse_are_shared_including_closed_child_contexts() {
    let mut p = process_with_pages(64);
    let handle = child(&mut p);
    for index in 0..64 {
        assert_eq!(call(&mut p, ALLOC, &[]), index);
        call(&mut p, SET, &[index, index + 100]);
    }
    p.memory
        .write(u64::from(PRIMARY + 0x34), &77_u32.to_le_bytes())
        .unwrap();
    p.cpu.set_fs_base(CHILD);
    for index in [0, 31, 63] {
        assert_eq!(call(&mut p, GET, &[index]), 0);
        call(&mut p, SET, &[index, index + 200]);
    }
    assert_eq!(call(&mut p, ALLOC, &[]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 259);
    assert_eq!(call(&mut p, 0x7000_021c, &[handle]), 1);
    assert_eq!(call(&mut p, GET, &[31]), 231);
    assert_eq!(call(&mut p, FREE, &[31]), 1);
    call(&mut p, SET, &[31, 333]);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(call(&mut p, GET, &[31]), 0);
    call(&mut p, SET, &[31, 444]);
    assert_eq!(call(&mut p, ALLOC, &[]), 31);
    assert_eq!(call(&mut p, GET, &[31]), 0);
    assert_eq!(call(&mut p, GET, &[0]), 100);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, GET, &[31]), 0);
    assert_eq!(call(&mut p, GET, &[0]), 200);
    assert_eq!(call(&mut p, GET, &[63]), 263);
}

#[test]
fn unknown_fs_bases_and_failed_creation_cannot_register_tls_tables() {
    let mut p = process_with_pages(41);
    call(&mut p, ALLOC, &[]);
    call(&mut p, SET, &[0, 42]);
    prepare(&mut p, CREATE, &[0, 0, 0x0040_1000, 0, 4, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .map_zeroed(0x5000_0000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for base in [CHILD, 0x5000_0000, u32::MAX] {
        p.cpu.set_fs_base(base);
        for (api, args) in [
            (ALLOC, vec![]),
            (FREE, vec![0]),
            (GET, vec![0]),
            (SET, vec![0, 99]),
        ] {
            prepare(&mut p, api, &args);
            let before = p.cpu;
            let result = p.run(1);
            assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
    }
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, GET, &[0]), 42);
    assert_eq!(call(&mut p, ALLOC, &[]), 1);
}

#[test]
fn child_error_faults_preserve_values_and_successful_mutations_need_no_teb_access() {
    let mut p = process_with_pages(64);
    child(&mut p);
    call(&mut p, ALLOC, &[]);
    call(&mut p, SET, &[0, 111]);
    p.cpu.set_fs_base(CHILD);
    call(&mut p, SET, &[0, 222]);
    p.memory
        .write(u64::from(CHILD + 0x34), &55_u32.to_le_bytes())
        .unwrap();
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (api, args) in [(GET, vec![0]), (FREE, vec![63]), (SET, vec![64, 99])] {
        prepare(&mut p, api, &args);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, ALLOC, &[]), 1);
    assert_eq!(call(&mut p, SET, &[1, 333]), 1);
    assert_eq!(call(&mut p, FREE, &[1]), 1);
    for index in 1..64 {
        assert_eq!(call(&mut p, ALLOC, &[]), index);
    }
    prepare(&mut p, ALLOC, &[]);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 55);
    assert_eq!(call(&mut p, GET, &[0]), 222);
    assert_eq!(call(&mut p, GET, &[1]), 0);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, GET, &[0]), 111);
}

#[test]
fn all_slots_exhaust_and_reuse_with_zero_values() {
    let mut p = process();
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    assert_eq!(call(&mut p, SET, &[0, u32::MAX]), 1);
    for index in 0..64 {
        assert_eq!(call(&mut p, ALLOC, &[]), index);
        assert_eq!(call(&mut p, GET, &[index]), 0);
        assert_eq!(call(&mut p, SET, &[index, index + 100]), 1);
    }
    assert_eq!(call(&mut p, ALLOC, &[]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 259);
    assert_eq!(call(&mut p, FREE, &[31]), 1);
    assert_eq!(p.last_error().unwrap(), 259);
    assert_eq!(call(&mut p, ALLOC, &[]), 31);
    assert_eq!(call(&mut p, GET, &[31]), 0);
    for index in [0, 30, 32, 63] {
        assert_eq!(call(&mut p, GET, &[index]), index + 100);
    }
    assert_eq!(p.last_error().unwrap(), 0);
}

#[test]
fn values_are_opaque_and_range_checks_are_independent_of_allocation() {
    let mut p = process();
    for index in [0, 63] {
        for value in [0, 0x0040_2180, u32::MAX] {
            assert_eq!(call(&mut p, SET, &[index, value]), 1);
            assert_eq!(call(&mut p, GET, &[index]), value);
        }
    }
    for index in [64, u32::MAX] {
        for (api, args) in [
            (GET, vec![index]),
            (SET, vec![index, 42]),
            (FREE, vec![index]),
        ] {
            assert_eq!(call(&mut p, api, &args), 0);
            assert_eq!(p.last_error().unwrap(), 87);
        }
    }
    assert_eq!(call(&mut p, FREE, &[0]), 0);
    assert_eq!(p.last_error().unwrap(), 87);
    assert_eq!(call(&mut p, ALLOC, &[]), 0);
    p.memory.write(0x0040_2180, &[42]).unwrap();
    call(&mut p, SET, &[0, 0x0040_2180]);
    assert_eq!(call(&mut p, FREE, &[0]), 1);
    let mut byte = [0];
    p.memory.read(0x0040_2180, &mut byte).unwrap();
    assert_eq!(byte, [42]);
    assert_eq!(call(&mut p, GET, &[0]), 0);
    let mut separate = process();
    assert_eq!(call(&mut separate, GET, &[63]), 0);
    assert_eq!(call(&mut separate, ALLOC, &[]), 0);
}

#[test]
fn last_error_and_frame_faults_preserve_tls_and_cpu_state() {
    let mut p = process();
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, ALLOC, &[]), 0);
    assert_eq!(call(&mut p, SET, &[0, 42]), 1);
    assert_eq!(call(&mut p, FREE, &[0]), 1);
    assert_eq!(call(&mut p, ALLOC, &[]), 0);
    call(&mut p, SET, &[0, 42]);
    for (api, args) in [(GET, vec![0]), (FREE, vec![63]), (SET, vec![64, 99])] {
        prepare(&mut p, api, &args);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for index in 1..64 {
        assert_eq!(call(&mut p, ALLOC, &[]), index);
    }
    prepare(&mut p, ALLOC, &[]);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 99);
    assert_eq!(call(&mut p, GET, &[0]), 42);
    assert_eq!(call(&mut p, FREE, &[0]), 1);
    prepare(&mut p, ALLOC, &[]);
    p.cpu.set_register(Register32::Esp, u32::MAX - 2);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, ALLOC, &[]), 0);
}
