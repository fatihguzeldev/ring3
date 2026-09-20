#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Access, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const ALLOC: u32 = 0x7000_0078;
const LOCK: u32 = 0x7000_007c;
const UNLOCK: u32 = 0x7000_0080;
const FREE: u32 = 0x7000_0084;
const STACK: u32 = 0x1000_ff00;
const ERROR: u64 = 0x7ffd_e034;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xcc],
            "kernel32.dll",
            &["GlobalAlloc", "GlobalLock", "GlobalUnlock", "GlobalFree"],
        ),
        limit,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    for (index, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(process, api, args);
    let flags = process.cpu.eflags;
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(
        process.cpu.register(Register32::Esp),
        STACK + (u32::try_from(args.len()).unwrap() + 1) * 4
    );
    assert_eq!(process.cpu.eflags, flags);
    process.cpu.register(Register32::Eax)
}

#[test]
fn movable_blocks_hold_real_data_across_nested_locks_and_locked_free() {
    let mut process = process(28);
    let initial = process.memory.mapped_pages();
    process.memory.write(ERROR, &99_u32.to_le_bytes()).unwrap();
    let handle = call(&mut process, ALLOC, &[0x7172, 4097]);
    assert_ne!(handle, 0);
    let pointer = call(&mut process, LOCK, &[handle]);
    assert_ne!(pointer, 0);
    assert_ne!(pointer, handle);
    assert_eq!(pointer % 8, 0);
    assert_eq!(process.memory.mapped_pages(), initial + 2);
    let mut data = [1; 4097];
    process.memory.read(u64::from(pointer), &mut data).unwrap();
    assert_eq!(data, [0; 4097]);
    process
        .memory
        .write(u64::from(pointer) + 4096, &[42])
        .unwrap();
    assert!(matches!(
        process.memory.fetch(u64::from(pointer), &mut [0]),
        Err(MemoryError::PermissionDenied {
            access: Access::Execute,
            ..
        })
    ));
    assert_eq!(call(&mut process, LOCK, &[handle]), pointer);
    assert_eq!(call(&mut process, UNLOCK, &[handle]), 1);
    assert_eq!(process.last_error().unwrap(), 99);
    assert_eq!(call(&mut process, UNLOCK, &[handle]), 0);
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(call(&mut process, UNLOCK, &[handle]), 0);
    assert_eq!(process.last_error().unwrap(), 158);
    assert_eq!(call(&mut process, LOCK, &[handle]), pointer);
    process
        .memory
        .read(u64::from(pointer) + 4096, &mut data[..1])
        .unwrap();
    assert_eq!(data[0], 42);
    assert_eq!(call(&mut process, FREE, &[handle]), 0);
    assert_eq!(process.memory.mapped_pages(), initial);
    assert!(matches!(
        process.memory.read(u64::from(pointer), &mut [0]),
        Err(MemoryError::Unmapped { .. })
    ));
    assert_eq!(call(&mut process, LOCK, &[handle]), 0);
    assert_eq!(process.last_error().unwrap(), 6);
    let replacement = call(&mut process, ALLOC, &[2, 4097]);
    let replacement_pointer = call(&mut process, LOCK, &[replacement]);
    process
        .memory
        .read(u64::from(replacement_pointer) + 4096, &mut data[..1])
        .unwrap();
    assert_eq!(data[0], 0);
}

#[test]
fn fixed_discarded_and_wrong_family_handles_follow_distinct_lifecycles() {
    let mut process = process(32);
    let fixed = call(&mut process, ALLOC, &[0x40, 0]);
    assert_ne!(fixed, 0);
    assert_eq!(call(&mut process, LOCK, &[fixed]), fixed);
    assert_eq!(call(&mut process, UNLOCK, &[fixed]), 1);
    let discarded = call(&mut process, ALLOC, &[2, 0]);
    assert_ne!(discarded, 0);
    assert_eq!(call(&mut process, LOCK, &[discarded]), 0);
    assert_eq!(process.last_error().unwrap(), 157);
    assert_eq!(call(&mut process, UNLOCK, &[discarded]), 0);
    assert_eq!(process.last_error().unwrap(), 158);
    let local = call(&mut process, 0x7000_0028, &[0, 1]);
    for invalid in [0, local, fixed + 1, u32::MAX] {
        assert_eq!(call(&mut process, LOCK, &[invalid]), 0);
        assert_eq!(process.last_error().unwrap(), 6);
        assert_eq!(call(&mut process, UNLOCK, &[invalid]), 0);
        assert_eq!(process.last_error().unwrap(), 6);
        assert_eq!(call(&mut process, FREE, &[invalid]), invalid);
    }
    for handle in [fixed, discarded] {
        assert_eq!(call(&mut process, 0x7000_002c, &[handle]), handle);
        assert_eq!(process.last_error().unwrap(), 6);
        assert_eq!(call(&mut process, FREE, &[handle]), 0);
    }
    assert_eq!(call(&mut process, 0x7000_002c, &[local]), 0);
}

#[test]
fn allocation_failures_and_final_unlock_error_write_are_atomic() {
    let mut process = process(26);
    let handle = call(&mut process, ALLOC, &[2, 1]);
    assert_ne!(call(&mut process, LOCK, &[handle]), 0);
    let pages = process.memory.mapped_pages();
    for size in [0, 1, u32::MAX] {
        assert_eq!(call(&mut process, ALLOC, &[2, size]), 0);
        assert_eq!(process.last_error().unwrap(), 8);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for (api, args) in [
        (ALLOC, vec![2, 1]),
        (UNLOCK, vec![handle]),
        (LOCK, vec![0]),
        (FREE, vec![handle + 1]),
    ] {
        prepare(&mut process, api, &args);
        let before = process.cpu;
        let run = process.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut process, UNLOCK, &[handle]), 0);
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(call(&mut process, FREE, &[handle]), 0);
    assert_ne!(call(&mut process, ALLOC, &[2, 0]), 0);
}

#[test]
fn unsupported_flags_and_self_frame_release_preserve_allocations() {
    let mut process = process(32);
    let pages = process.memory.mapped_pages();
    for flags in [1, 0x80, u32::MAX] {
        prepare(&mut process, ALLOC, &[flags, 16]);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: ALLOC }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
    }
    prepare(&mut process, ALLOC, &[2, 16]);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
    let handle = call(&mut process, ALLOC, &[2, 16]);
    let pointer = call(&mut process, LOCK, &[handle]);
    process.cpu.eip = FREE;
    process.cpu.set_register(Register32::Esp, pointer);
    process
        .memory
        .write(u64::from(pointer), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(pointer) + 4, &handle.to_le_bytes())
        .unwrap();
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: FREE }
    );
    assert_eq!(process.cpu, before);
    assert_eq!(call(&mut process, UNLOCK, &[handle]), 0);
    assert_eq!(call(&mut process, FREE, &[handle]), 0);
}
