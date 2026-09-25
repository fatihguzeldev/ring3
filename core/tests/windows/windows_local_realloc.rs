use super::imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const BASE: u32 = 0x2000_0000;
const ALLOC: u32 = 0x7000_0028;
const FREE: u32 = 0x7000_002c;
const REALLOC: u32 = 0x7000_00d4;
const STACK: u32 = 0x1000_ff00;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["LocalReAlloc"]),
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
    let before = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    let mut expected = before;
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(arguments.len() + 1).unwrap() * 4,
    );
    assert_eq!(process.cpu, expected);
    value
}

fn bytes(process: &Process32, pointer: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    process
        .memory
        .read(u64::from(pointer), &mut output)
        .unwrap();
    output
}

#[test]
fn resize_tracks_logical_size_across_same_page_and_page_boundaries() {
    let mut p = process(40);
    p.memory.write(0x7ffd_e034, &99_u32.to_le_bytes()).unwrap();
    let initial = p.memory.mapped_pages();
    let ptr = call(&mut p, ALLOC, &[0, 5]);
    p.memory.write(u64::from(ptr), &[7; 4096]).unwrap();
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8, 0]), ptr);
    assert_eq!(bytes(&p, ptr, 10), [7; 10]);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 3, 2]), ptr);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8, 0x40]), ptr);
    assert_eq!(bytes(&p, ptr, 10), [7, 7, 7, 0, 0, 0, 0, 0, 7, 7]);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8193, 0x40]), ptr);
    assert_eq!(p.memory.mapped_pages(), initial + 3);
    assert_eq!(bytes(&p, ptr + 3, 8190), vec![0; 8190]);
    p.memory.write(u64::from(ptr + 4095), &[42, 43]).unwrap();
    assert_eq!(call(&mut p, REALLOC, &[ptr, 4096, 0]), ptr);
    assert_eq!(bytes(&p, ptr + 4095, 1), [42]);
    assert!(p.memory.read(u64::from(ptr + 4096), &mut [0]).is_err());
    assert_eq!(p.memory.mapped_pages(), initial + 1);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 0, 0]), ptr);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 4, 0x42]), ptr);
    assert_eq!(bytes(&p, ptr, 4), [0; 4]);
    assert_eq!(call(&mut p, FREE, &[ptr]), 0);
    assert_eq!(p.memory.mapped_pages(), initial);
    assert_eq!(p.last_error().unwrap(), 99);
}

#[test]
fn relocation_requires_permission_and_preserves_content_and_ownership() {
    let mut p = process(40);
    let ptr = call(&mut p, ALLOC, &[0, 4097]);
    let blocker = call(&mut p, ALLOC, &[0, 1]);
    let data: Vec<u8> = (0..4097).map(|i| u8::try_from(i % 251).unwrap()).collect();
    p.memory.write(u64::from(ptr), &data).unwrap();
    p.memory.write(u64::from(blocker), &[91]).unwrap();
    let mapped = p.memory.mapped_pages();
    assert_eq!(call(&mut p, REALLOC, &[ptr, 12289, 0]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(bytes(&p, ptr, data.len()), data);
    assert_eq!(p.memory.mapped_pages(), mapped);
    let moved = call(&mut p, REALLOC, &[ptr, 12289, 0x42]);
    assert_eq!(moved, BASE + 12288);
    assert_eq!(bytes(&p, moved, data.len()), data);
    assert_eq!(bytes(&p, moved + 4097, 8192), vec![0; 8192]);
    assert_eq!(bytes(&p, blocker, 1), [91]);
    assert!(p.memory.read(u64::from(ptr), &mut [0]).is_err());
    assert_eq!(p.memory.mapped_pages(), mapped + 2);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 1, 0]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, FREE, &[moved]), 0);
    assert_eq!(call(&mut p, ALLOC, &[0, 8192]), ptr);
}

#[test]
fn invalid_owners_flags_and_capacity_fail_without_losing_the_old_block() {
    let mut p = process(28);
    let ptr = call(&mut p, ALLOC, &[0, 4]);
    let global = call(&mut p, 0x7000_0078, &[0, 1]);
    prepare(&mut p, 0x7000_011c, &[1]);
    assert_eq!(p.run(1).api_calls, 1);
    let crt = p.cpu.register(Register32::Eax);
    for invalid in [0, ptr + 1, global, crt, u32::MAX] {
        assert_eq!(call(&mut p, REALLOC, &[invalid, 8, 0]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
    }
    let mapped = p.memory.mapped_pages();
    p.memory.write(u64::from(ptr), &[9; 8]).unwrap();
    for size in [8192, 0x1000_0001, u32::MAX] {
        assert_eq!(call(&mut p, REALLOC, &[ptr, size, 2]), 0);
        assert_eq!(p.last_error().unwrap(), 8);
        assert_eq!(p.memory.mapped_pages(), mapped);
        assert_eq!(bytes(&p, ptr, 8), [9; 8]);
    }
    for flags in [1, 0x80, 0x100, u32::MAX] {
        prepare(&mut p, REALLOC, &[ptr, 8, flags]);
        let before = p.cpu;
        let result = p.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: REALLOC }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), mapped);
    }
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8, 0x40]), ptr);
    assert_eq!(bytes(&p, ptr, 8), [9, 9, 9, 9, 0, 0, 0, 0]);
    let mut other = process(28);
    assert_eq!(call(&mut other, REALLOC, &[ptr, 8, 0]), 0);
}

#[test]
fn zero_fill_copy_and_argument_faults_preserve_bytes_pages_cpu_and_size() {
    let mut p = process(40);
    let ptr = call(&mut p, ALLOC, &[0, 4097]);
    p.memory.write(u64::from(ptr), &[9; 8192]).unwrap();
    p.memory
        .protect(u64::from(ptr + 4096), 4096, Permissions::READ)
        .unwrap();
    for size in [8192, 12288] {
        prepare(&mut p, REALLOC, &[ptr, size, 0x40]);
        let before = p.cpu;
        let mapped = p.memory.mapped_pages();
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), mapped);
        assert_eq!(bytes(&p, ptr, 8192), vec![9; 8192]);
    }
    call(&mut p, ALLOC, &[0, 1]);
    p.memory
        .protect(u64::from(ptr + 4096), 4096, Permissions::NONE)
        .unwrap();
    prepare(&mut p, REALLOC, &[ptr, 12288, 2]);
    let before = p.cpu;
    let mapped = p.memory.mapped_pages();
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), mapped);
    p.memory
        .protect(u64::from(ptr + 4096), 4096, Permissions::READ_WRITE)
        .unwrap();
    for stack in [0x1000_fff4, u32::MAX - 11] {
        prepare(&mut p, REALLOC, &[ptr, 4, 0]);
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8192, 0x40]), ptr);
    assert_eq!(bytes(&p, ptr, 4097), vec![9; 4097]);
    assert_eq!(bytes(&p, ptr + 4097, 4095), vec![0; 4095]);
}

#[test]
fn error_write_faults_and_active_frames_cannot_mutate_allocations() {
    let mut p = process(30);
    let ptr = call(&mut p, ALLOC, &[0, 8192]);
    p.memory
        .map_zeroed(u64::from(ptr - 4096), 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(u64::from(ptr + 8192), 4096, Permissions::READ_WRITE)
        .unwrap();
    for stack in [ptr - 12, ptr, ptr + 8192 - 4] {
        let args = [0x0040_1000, ptr, 0, 0];
        for (i, value) in args.iter().enumerate() {
            p.memory
                .write(u64::from(stack) + i as u64 * 4, &value.to_le_bytes())
                .unwrap();
        }
        p.cpu.eip = REALLOC;
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let snapshot = bytes(&p, ptr - 4096, 16384);
        let result = p.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: REALLOC }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, ptr - 4096, 16384), snapshot);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    for args in [[0, 1, 0], [ptr, u32::MAX, 2]] {
        prepare(&mut p, REALLOC, &args);
        let before = p.cpu;
        let mapped = p.memory.mapped_pages();
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), mapped);
    }
    assert_eq!(call(&mut p, FREE, &[ptr]), 0);
}

use super::local_realloc_executable;

#[test]
fn resizing_guest_matches_whole_and_single_instruction_execution() {
    let bytes = local_realloc_executable::pe32();
    let mut whole = Process32::load(&bytes, 40).unwrap();
    let mut stepped = Process32::load(&bytes, 40).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 3));
    let (mut instructions, mut apis) = (0, 0);
    loop {
        let result = stepped.run(1);
        instructions += result.instructions;
        apis += result.api_calls;
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!((instructions, apis), (15, 3));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.cpu.register(Register32::Ebx), BASE);
    assert_eq!(whole.cpu.register(Register32::Edi), BASE);
    assert_eq!(whole.cpu.register(Register32::Ecx), 42);
    assert_eq!(whole.cpu.register(Register32::Edx), 0);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(whole.memory.mapped_pages(), stepped.memory.mapped_pages());
    assert!(whole.memory.read(u64::from(BASE), &mut [0]).is_err());
}

#[test]
fn in_place_capacity_failure_preserves_size_and_relocation_uses_fresh_permissions() {
    let mut p = process(26);
    let ptr = call(&mut p, ALLOC, &[0, 1]);
    p.memory.write(u64::from(ptr), &[9; 8]).unwrap();
    assert_eq!(call(&mut p, REALLOC, &[ptr, 4097, 0x42]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(bytes(&p, ptr, 8), [9; 8]);
    assert_eq!(call(&mut p, REALLOC, &[ptr, 8, 0x40]), ptr);
    assert_eq!(bytes(&p, ptr, 8), [9, 0, 0, 0, 0, 0, 0, 0]);
    let mut p = process(32);
    let ptr = call(&mut p, ALLOC, &[0, 1]);
    call(&mut p, ALLOC, &[0, 1]);
    p.memory.write(u64::from(ptr), &[42]).unwrap();
    p.memory
        .protect(u64::from(ptr), 4096, Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(call(&mut p, REALLOC, &[ptr, 1, 0]), ptr);
    assert!(p.memory.write(u64::from(ptr), &[3]).is_err());
    let moved = call(&mut p, REALLOC, &[ptr, 4097, 2]);
    assert_ne!(moved, ptr);
    assert_eq!(bytes(&p, moved, 1), [42]);
    p.memory.write(u64::from(moved), &[7]).unwrap();
    assert!(p.memory.fetch(u64::from(moved), &mut [0]).is_err());
}
