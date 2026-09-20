#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const ALLOC: u32 = 0x7000_0068;
const FREE: u32 = 0x7000_006c;
const GET: u32 = 0x7000_0070;
const SET: u32 = 0x7000_0074;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "kernel32.dll",
            &["TlsAlloc", "TlsFree", "TlsGetValue", "TlsSetValue"],
        ),
        32,
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
        STACK + (u32::try_from(arguments.len()).unwrap() + 1) * 4
    );
    p.cpu.register(Register32::Eax)
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
