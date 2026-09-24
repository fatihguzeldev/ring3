#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const MALLOC: u32 = 0x7000_011c;
const FREE: u32 = 0x7000_0120;
const ERRNO: u32 = 0x7000_0124;
const NEW: u32 = 0x7000_0144;
const DELETE: u32 = 0x7000_0148;
const ERROR: u64 = 0x7000_2020;
const STACK: u32 = 0x1000_ff00;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["malloc", "free", "_errno"]),
        limit,
    )
    .unwrap()
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
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.eip, 0x0040_1000);
    let cleanup = if [MALLOC, FREE, ERRNO, NEW, DELETE].contains(&api) {
        4
    } else {
        u32::try_from((args.len() + 1) * 4).unwrap()
    };
    assert_eq!(process.cpu.register(Register32::Esp), STACK + cleanup);
    assert_eq!(process.cpu.eflags, 0xced7);
    process.cpu.register(Register32::Eax)
}

#[test]
fn legacy_new_and_delete_own_guest_memory_and_preserve_errors_even_on_exhaustion() {
    let mut process = process(28);
    let initial = process.memory.mapped_pages();
    process.memory.write(ERROR, &123_u32.to_le_bytes()).unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    let pointer = call(&mut process, NEW, &[4097]);
    assert_ne!(pointer, 0);
    assert_eq!(pointer % 8, 0);
    process
        .memory
        .write(u64::from(pointer) + 4096, &[42])
        .unwrap();
    assert!(process.memory.fetch(u64::from(pointer), &mut [0]).is_err());
    let zero = call(&mut process, NEW, &[0]);
    assert_ne!(zero, 0);
    assert_ne!(zero, pointer);
    for size in [2049, 4096, u32::MAX] {
        assert_eq!(call(&mut process, NEW, &[size]), 0);
        assert_eq!(process.memory.mapped_pages(), initial + 3);
        assert_eq!(read(&process, ERROR), 123);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, NEW, &[2049]), 0);
    assert_eq!(call(&mut process, DELETE, &[pointer]), 99);
    assert_eq!(call(&mut process, NEW, &[4097]), pointer);
    assert_eq!(call(&mut process, DELETE, &[pointer]), 99);
    assert_eq!(call(&mut process, DELETE, &[zero]), 99);
    assert_eq!(call(&mut process, DELETE, &[0]), 99);
    assert_eq!(process.memory.mapped_pages(), initial);
}

fn read(process: &Process32, address: u64) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(address, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn crt_allocations_are_writable_freeable_and_keep_errno_and_last_error_separate() {
    let mut process = process(28);
    let initial = process.memory.mapped_pages();
    assert_eq!(u64::from(call(&mut process, ERRNO, &[])), ERROR);
    assert_eq!(read(&process, ERROR), 0);
    process.memory.write(ERROR, &123_u32.to_le_bytes()).unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    let pointer = call(&mut process, MALLOC, &[4097]);
    assert_ne!(pointer, 0);
    assert_eq!(pointer % 8, 0);
    process
        .memory
        .write(u64::from(pointer) + 4096, &[42])
        .unwrap();
    assert!(process.memory.fetch(u64::from(pointer), &mut [0]).is_err());
    let zero = call(&mut process, MALLOC, &[0]);
    assert_ne!(zero, 0);
    assert_eq!(process.memory.mapped_pages(), initial + 3);
    assert_eq!(call(&mut process, FREE, &[pointer]), 99);
    assert!(process.memory.read(u64::from(pointer), &mut [0]).is_err());
    assert_eq!(call(&mut process, MALLOC, &[4097]), pointer);
    assert_eq!(call(&mut process, FREE, &[pointer]), 99);
    assert_eq!(call(&mut process, FREE, &[zero]), 99);
    assert_eq!(call(&mut process, FREE, &[0]), 99);
    assert_eq!(process.memory.mapped_pages(), initial);
    assert_eq!(read(&process, ERROR), 123);
    assert_eq!(process.last_error().unwrap(), 77);
}

#[test]
fn all_cross_family_operations_refuse_crt_and_windows_ownership_mismatches() {
    let mut process = process(32);
    let pointer = call(&mut process, MALLOC, &[1]);
    let local = call(&mut process, 0x7000_0028, &[0, 1]);
    let fixed = call(&mut process, 0x7000_0078, &[0, 1]);
    let movable = call(&mut process, 0x7000_0078, &[2, 1]);
    let pages = process.memory.mapped_pages();
    for api in [FREE, DELETE] {
        for invalid in [local, fixed, movable, pointer + 1, u32::MAX] {
            prepare(&mut process, api, &[invalid]);
            let before = process.cpu;
            assert_eq!(
                process.run(1).reason,
                ProcessStop::UnsupportedApi { address: api }
            );
            assert_eq!(process.cpu, before);
            assert_eq!(process.memory.mapped_pages(), pages);
        }
    }
    for api in [0x7000_002c, 0x7000_0084, 0x7000_007c, 0x7000_0080] {
        assert_eq!(
            call(&mut process, api, &[pointer]),
            if [0x7000_002c, 0x7000_0084].contains(&api) {
                pointer
            } else {
                0
            }
        );
        assert_eq!(process.last_error().unwrap(), 6);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    assert_eq!(call(&mut process, FREE, &[pointer]), 99);
    prepare(&mut process, FREE, &[pointer]);
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: FREE }
    );
}

#[test]
fn exhaustion_sets_only_errno_and_a_faulting_errno_write_leaves_capacity_intact() {
    let mut process = process(26);
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    let pointer = call(&mut process, MALLOC, &[1]);
    let pages = process.memory.mapped_pages();
    for size in [2049, 4096, u32::MAX] {
        assert_eq!(call(&mut process, MALLOC, &[size]), 0);
        assert_eq!(read(&process, ERROR), 12);
        assert_eq!(process.last_error().unwrap(), 77);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    prepare(&mut process, MALLOC, &[2049]);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
    assert_eq!(call(&mut process, FREE, &[pointer]), 99);
    assert_eq!(call(&mut process, MALLOC, &[1]), pointer);
    assert_eq!(u64::from(call(&mut process, ERRNO, &[])), ERROR);
}

#[test]
fn invalid_cdecl_frame_and_freeing_its_backing_preserve_heap_state() {
    let mut process = process(27);
    let pointer = call(&mut process, MALLOC, &[4097]);
    let pages = process.memory.mapped_pages();
    for api in [MALLOC, NEW] {
        prepare(&mut process, api, &[1]);
        process.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    for api in [FREE, DELETE] {
        process.cpu.eip = api;
        process.cpu.set_register(Register32::Esp, pointer);
        process
            .memory
            .write(u64::from(pointer), &0x0040_1000_u32.to_le_bytes())
            .unwrap();
        process
            .memory
            .write(u64::from(pointer) + 4, &pointer.to_le_bytes())
            .unwrap();
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: api }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    assert_eq!(call(&mut process, FREE, &[pointer]), 99);
}
