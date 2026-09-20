#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Access, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const BASE: u32 = 0x2000_0000;
const ALLOC: u32 = 0x7000_0028;
const FREE: u32 = 0x7000_002c;
const STACK: u32 = 0x1000_ff00;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "kernel32.dll", &["LocalAlloc", "LocalFree"]),
        limit,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, api: u32, arguments: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    for (index, value) in std::iter::once(&0x0040_1000).chain(arguments).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(process: &mut Process32, api: u32, arguments: &[u32]) -> u32 {
    prepare(process, api, arguments);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(
        process.cpu.register(Register32::Esp),
        STACK + (u32::try_from(arguments.len()).unwrap() + 1) * 4
    );
    process.cpu.register(Register32::Eax)
}

#[test]
fn fixed_allocations_are_zeroed_writable_nonexecutable_and_reusable() {
    let mut process = process(28);
    let initial = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &99_u32.to_le_bytes())
        .unwrap();
    let pointer = call(&mut process, ALLOC, &[0x40, 4097]);
    assert_eq!(pointer, BASE);
    assert_eq!(process.memory.mapped_pages(), initial + 2);
    let mut bytes = [1; 4097];
    process.memory.read(u64::from(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4097]);
    process
        .memory
        .write(u64::from(pointer), &[7; 4097])
        .unwrap();
    assert_eq!(
        process.memory.fetch(u64::from(pointer), &mut [0]),
        Err(MemoryError::PermissionDenied {
            address: u64::from(pointer),
            access: Access::Execute
        })
    );
    let zero = call(&mut process, ALLOC, &[0, 0]);
    assert_eq!(zero, BASE + 8192);
    assert_eq!(call(&mut process, FREE, &[pointer]), 0);
    assert_eq!(
        process.memory.read(u64::from(pointer), &mut [0]),
        Err(MemoryError::Unmapped {
            address: u64::from(pointer)
        })
    );
    assert_eq!(call(&mut process, ALLOC, &[0x40, 8192]), pointer);
    process.memory.read(u64::from(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4097]);
    assert_eq!(call(&mut process, FREE, &[pointer]), 0);
    assert_eq!(call(&mut process, FREE, &[zero]), 0);
    assert_eq!(call(&mut process, FREE, &[0]), 0);
    assert_eq!(process.memory.mapped_pages(), initial);
    assert_eq!(process.last_error().unwrap(), 99);
}

#[test]
fn first_fit_skips_existing_pages_and_failed_allocations_do_not_consume_capacity() {
    let mut process = process(29);
    process
        .memory
        .map_zeroed(u64::from(BASE) + PAGE_SIZE, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut process, ALLOC, &[0, 8192]), BASE + 8192);
    assert_eq!(call(&mut process, ALLOC, &[0, 4096]), BASE);
    let mapped = process.memory.mapped_pages();
    for size in [1, u32::MAX, 0x1000_0001] {
        assert_eq!(call(&mut process, ALLOC, &[0, size]), 0);
        assert_eq!(process.last_error().unwrap(), 8);
        assert_eq!(process.memory.mapped_pages(), mapped);
    }
    assert_eq!(call(&mut process, FREE, &[BASE]), 0);
    assert_eq!(call(&mut process, ALLOC, &[0, 4096]), BASE);
    assert_eq!(process.last_error().unwrap(), 8);
}

#[test]
fn invalid_free_preserves_allocations_and_valid_free_ignores_page_permissions() {
    let mut process = process(32);
    let pointer = call(&mut process, ALLOC, &[0, 8192]);
    let count = process.memory.mapped_pages();
    for invalid in [pointer + 1, pointer + 4096, 0x0040_0000, u32::MAX] {
        assert_eq!(call(&mut process, FREE, &[invalid]), invalid);
        assert_eq!(process.last_error().unwrap(), 6);
        assert_eq!(process.memory.mapped_pages(), count);
    }
    process
        .memory
        .protect(u64::from(pointer), PAGE_SIZE * 2, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, FREE, &[pointer]), 0);
    assert_eq!(call(&mut process, FREE, &[pointer]), pointer);
    assert_eq!(process.memory.mapped_pages(), count - 2);
    let mut separate = self::process(32);
    assert_eq!(call(&mut separate, FREE, &[pointer]), pointer);
}

#[test]
fn unsupported_flags_and_faulting_frames_preserve_cpu_and_memory() {
    let mut process = process(32);
    let count = process.memory.mapped_pages();
    for flags in [2, 0x42, 0x100, u32::MAX] {
        prepare(&mut process, ALLOC, &[flags, 10]);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: ALLOC }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 0);
        assert_eq!(process.memory.mapped_pages(), count);
    }
    for stack in [0x1000_fffc, u32::MAX - 3] {
        prepare(&mut process, ALLOC, &[0, 10]);
        process.cpu.set_register(Register32::Esp, stack);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), count);
    }
    assert_eq!(call(&mut process, ALLOC, &[0, 10]), BASE);
}

#[test]
fn last_error_write_faults_do_not_consume_or_free_heap_state() {
    let mut process = process(26);
    let pointer = call(&mut process, ALLOC, &[0, 1]);
    let count = process.memory.mapped_pages();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for (api, args) in [(ALLOC, vec![0, 1]), (FREE, vec![pointer + 1])] {
        prepare(&mut process, api, &args);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), count);
    }
    assert_eq!(call(&mut process, FREE, &[pointer]), 0);
    assert_eq!(call(&mut process, ALLOC, &[0, 1]), pointer);
}

#[test]
fn freeing_active_api_frame_is_rejected_before_unmapping() {
    let mut process = process(32);
    let pointer = call(&mut process, ALLOC, &[0, 8192]);
    process
        .memory
        .map_zeroed(
            u64::from(pointer) - PAGE_SIZE,
            PAGE_SIZE,
            Permissions::READ_WRITE,
        )
        .unwrap();
    for stack in [pointer - 4, pointer, pointer + 8192 - 8] {
        process
            .memory
            .write(u64::from(stack), &0x0040_1000_u32.to_le_bytes())
            .unwrap();
        process
            .memory
            .write(u64::from(stack) + 4, &pointer.to_le_bytes())
            .unwrap();
        process.cpu.eip = FREE;
        process.cpu.set_register(Register32::Esp, stack);
        let before = process.cpu;
        let count = process.memory.mapped_pages();
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: FREE }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), count);
    }
    // a return target in the block is allowed; subsequent execution faults normally.
    prepare(&mut process, FREE, &[pointer]);
    process
        .memory
        .write(u64::from(STACK), &pointer.to_le_bytes())
        .unwrap();
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.eip, pointer);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped { .. }))
    ));
}
