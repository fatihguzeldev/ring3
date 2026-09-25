use super::imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const DUPLICATE: u32 = 0x7000_0134;
const FREE: u32 = 0x7000_0120;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_strdup", "free"]),
        limit,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, api: u32, argument: u32) {
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
        .write(u64::from(STACK) + 4, &argument.to_le_bytes())
        .unwrap();
}

fn call(process: &mut Process32, api: u32, argument: u32) -> u32 {
    prepare(process, api, argument);
    let mut expected = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + if api == 0x7000_002c { 8 } else { 4 },
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
    value
}

fn read(process: &Process32, pointer: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(pointer), &mut bytes).unwrap();
    bytes
}

#[test]
fn duplicates_are_independent_writable_crt_allocations_with_exact_nul_content() {
    let mut process = process(32);
    process
        .memory
        .write(u64::from(SOURCE), b"a\x80\xff\0tail")
        .unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    process
        .memory
        .write(0x7000_2020, &123_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    let pages = process.memory.mapped_pages();
    let first = call(&mut process, DUPLICATE, SOURCE);
    let second = call(&mut process, DUPLICATE, first);
    let empty = call(&mut process, DUPLICATE, SOURCE + 3);
    assert_eq!(first, 0x2000_0000);
    assert_ne!(second, first);
    assert_ne!(empty, first);
    assert_ne!(empty, second);
    assert_eq!(process.memory.mapped_pages(), pages + 1);
    assert_eq!(read(&process, first, 5), b"a\x80\xff\0\0");
    assert_eq!(read(&process, second, 4), b"a\x80\xff\0");
    assert_eq!(read(&process, empty, 1), [0]);
    process.memory.write(u64::from(first), b"z").unwrap();
    assert_eq!(read(&process, second, 4), b"a\x80\xff\0");
    assert_eq!(read(&process, SOURCE, 8), b"a\x80\xff\0tail");
    assert!(process.memory.fetch(u64::from(first), &mut [0]).is_err());
    assert_eq!(read(&process, 0x7000_2020, 4), 123_u32.to_le_bytes());
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(call(&mut process, 0x7000_002c, first), first);
    assert_eq!(process.last_error().unwrap(), 6);
    for pointer in [first, second, empty] {
        assert_eq!(call(&mut process, FREE, pointer), 99);
    }
    assert_eq!(process.memory.mapped_pages(), pages);
    for pointer in [first, second, empty] {
        assert!(process.memory.read(u64::from(pointer), &mut [0]).is_err());
    }
    assert_eq!(call(&mut process, DUPLICATE, SOURCE), first);
}

#[test]
fn exhaustion_sets_errno_and_error_write_faults_do_not_leak_heap_pages() {
    let mut process = process(26);
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    let first = call(&mut process, DUPLICATE, SOURCE);
    assert_ne!(first, 0);
    let pages = process.memory.mapped_pages();
    for _ in 0..255 {
        assert_ne!(call(&mut process, DUPLICATE, SOURCE), 0);
    }
    assert_eq!(process.memory.mapped_pages(), pages);
    assert_eq!(call(&mut process, DUPLICATE, SOURCE), 0);
    assert_eq!(read(&process, 0x7000_2020, 4), 12_u32.to_le_bytes());
    assert_eq!(process.last_error().unwrap(), 77);
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    prepare(&mut process, DUPLICATE, SOURCE);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
    assert_eq!(read(&process, first, 1), [0]);
    call(&mut process, FREE, first);
    assert_eq!(call(&mut process, DUPLICATE, SOURCE), first);
    assert_eq!(call(&mut process, DUPLICATE, 0), 0);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn invalid_sources_and_frames_leave_heap_and_cpu_unchanged() {
    for source in [0x0040_2ffe, 0x7000_0000, u32::MAX] {
        let mut process = process(32);
        if source == u32::MAX {
            process
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            process.memory.write(u64::from(source), b"x").unwrap();
        } else if source == 0x0040_2ffe {
            process.memory.write(u64::from(source), b"xx").unwrap();
        }
        let pages = process.memory.mapped_pages();
        prepare(&mut process, DUPLICATE, source);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert_eq!(call(&mut process, DUPLICATE, SOURCE), 0x2000_0000);
    }
    let mut process = process(26);
    prepare(&mut process, DUPLICATE, SOURCE);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    let pages = process.memory.mapped_pages();
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
    let pointer = call(&mut process, DUPLICATE, STACK + 4);
    assert_eq!(read(&process, pointer, 3), [4, 255, 0]);
}

#[test]
fn inclusive_scan_bound_and_top_address_nul_are_supported_without_extra_reads() {
    let mut process = process(64);
    process
        .memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .write(0x2000_0000, &vec![b'x'; 65536])
        .unwrap();
    let pages = process.memory.mapped_pages();
    prepare(&mut process, DUPLICATE, 0x2000_0000);
    let before = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::UnsupportedApi { address: DUPLICATE }
    );
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
    process.memory.write(0x2000_ffff, &[0]).unwrap();
    let pointer = call(&mut process, DUPLICATE, 0x2000_0000);
    assert_eq!(process.memory.mapped_pages(), pages + 16);
    assert_eq!(
        read(&process, pointer, 65536),
        read(&process, 0x2000_0000, 65536)
    );
    call(&mut process, FREE, pointer);
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let empty = call(&mut process, DUPLICATE, u32::MAX);
    assert_eq!(read(&process, empty, 1), [0]);
}
