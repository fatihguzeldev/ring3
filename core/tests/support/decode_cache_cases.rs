use ring3_core::execution::{
    Access, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const RWX: Permissions = Permissions {
    read: true,
    write: true,
    execute: true,
};

fn process(code: &[u8]) -> Process32 {
    Process32::load(&super::executable::pe32(code), 40).unwrap()
}

fn step(process: &mut Process32, ip: u32) {
    process.cpu.eip = ip;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (1, 0));
}

fn fault(process: &mut Process32, ip: u32, reason: StopReason) {
    process.cpu.eip = ip;
    let before = process.cpu;
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::Stopped(reason));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}

pub fn cached_execution_matches_uncached_and_observes_guest_writes() {
    let programs = [
        // a repeated arithmetic loop and a loop that modifies its own immediate.
        vec![
            0xb9, 0, 0x10, 0, 0, 0x43, 0x01, 0xd8, 0x49, 0x75, 0xfa, 0xcc,
        ],
        vec![
            0xb9, 3, 0, 0, 0, 0xb8, 1, 0, 0, 0, 0x83, 0x05, 6, 0x10, 0x40, 0, 1, 0x49, 0x75, 0xf1,
            0xcc,
        ],
    ];
    for code in programs {
        let mut cached = process(&code);
        let mut reference = process(&code);
        for p in [&mut cached, &mut reference] {
            p.memory.protect(0x0040_1000, PAGE_SIZE, RWX).unwrap();
        }
        for budget in [0, 1, 7, 4097, 20_000] {
            let expected = reference.cpu.run(&mut reference.memory, budget);
            let result = cached.run(budget);
            assert_eq!(result.reason, ProcessStop::Stopped(expected.reason.clone()));
            assert_eq!(
                (result.instructions, result.api_calls),
                (expected.instructions, 0)
            );
            assert_eq!(cached.cpu, reference.cpu);
            let mut actual_code = [0; 32];
            let mut expected_code = [0; 32];
            cached.memory.read(0x0040_1000, &mut actual_code).unwrap();
            reference
                .memory
                .read(0x0040_1000, &mut expected_code)
                .unwrap();
            assert_eq!(actual_code, expected_code);
            if expected.reason == StopReason::Breakpoint {
                break;
            }
        }
    }
}

pub fn cached_fetches_preserve_permissions_lengths_collisions_and_faults() {
    let mut p = process(&[0xcc]);
    p.memory
        .map_zeroed(0x6000_0000, 2 * PAGE_SIZE, RWX)
        .unwrap();
    for (ip, immediate) in [(0x6000_0000, 1), (0x6000_1000, 2), (0x6000_0000, 3)] {
        p.memory
            .write(u64::from(ip), &[0xb8, immediate, 0, 0, 0])
            .unwrap();
        step(&mut p, ip);
        step(&mut p, ip);
        assert_eq!(p.cpu.register(Register32::Eax), u32::from(immediate));
    }
    let ip = 0x6000_0ffe;
    p.memory
        .write(u64::from(ip), &[0xb8, 0x78, 0x56, 0x34, 0x12])
        .unwrap();
    step(&mut p, ip);
    p.memory
        .protect(0x6000_1000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    fault(
        &mut p,
        ip,
        StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: 0x6000_1000,
            access: Access::Execute,
        }),
    );
    p.memory.write(u64::from(ip), &[0xf0, 0x90]).unwrap();
    fault(&mut p, ip, StopReason::InvalidInstruction);
    p.memory.write(u64::from(ip), &[0x40, 0x78]).unwrap();
    step(&mut p, ip);
    assert_eq!(p.cpu.register(Register32::Eax), 0x1234_5679);
    p.memory.write(u64::from(ip), &[0xb8]).unwrap();
    fault(
        &mut p,
        ip,
        StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: 0x6000_1000,
            access: Access::Execute,
        }),
    );
    p.memory.protect(0x6000_1000, PAGE_SIZE, RWX).unwrap();
    p.memory.write(0x6000_1000, &[0xef, 0xcd, 0xab]).unwrap();
    step(&mut p, ip);
    assert_eq!(p.cpu.register(Register32::Eax), 0xabcd_ef78);
    p.memory
        .protect(0x6000_0000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    fault(
        &mut p,
        ip,
        StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(ip),
            access: Access::Execute,
        }),
    );
    p.memory.map_zeroed(0xffff_f000, PAGE_SIZE, RWX).unwrap();
    p.memory.write(u64::from(u32::MAX), &[0x40]).unwrap();
    for _ in 0..2 {
        step(&mut p, u32::MAX);
        assert_eq!(p.cpu.eip, 0);
    }
    p.memory.write(u64::from(u32::MAX), &[0xb8]).unwrap();
    fault(
        &mut p,
        u32::MAX,
        StopReason::MemoryFault(MemoryError::AddressOverflow),
    );
    p.cpu.eip = 0x5000_0000;
    let before = p.cpu;
    assert_eq!(p.run(0).instructions, 0);
    assert_eq!(p.cpu, before);
}

fn call(p: &mut Process32, api: u32, arguments: &[u32]) -> u32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, 0x1000_ff00);
    for (index, value) in std::iter::once(&0x0040_1000).chain(arguments).enumerate() {
        p.memory
            .write(0x1000_ff00 + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x0040_1000);
    p.cpu.register(Register32::Eax)
}

pub fn freed_and_remapped_code_never_uses_stale_decode() {
    let mut p = Process32::load(
        &super::imported_executable::pe32(&[0xcc], "kernel32.dll", &["LocalAlloc", "LocalFree"]),
        40,
    )
    .unwrap();
    let first = call(&mut p, 0x7000_0028, &[0, 4096]);
    let second = call(&mut p, 0x7000_0028, &[0, 4096]);
    assert_eq!(second, first + 4096);
    p.memory
        .protect(u64::from(first), 2 * PAGE_SIZE, RWX)
        .unwrap();
    let ip = first + 4094;
    p.memory.write(u64::from(ip), &[0xb8, 1, 0, 0, 0]).unwrap();
    step(&mut p, ip);
    assert_eq!(call(&mut p, 0x7000_002c, &[second]), 0);
    fault(
        &mut p,
        ip,
        StopReason::MemoryFault(MemoryError::Unmapped {
            address: u64::from(second),
        }),
    );
    p.memory.write(u64::from(ip), &[0x40]).unwrap();
    step(&mut p, ip);
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(call(&mut p, 0x7000_0028, &[0, 4096]), second);
    p.memory.protect(u64::from(second), PAGE_SIZE, RWX).unwrap();
    p.memory.write(u64::from(ip), &[0xb8, 2, 0, 0, 0]).unwrap();
    step(&mut p, ip);
    assert_eq!(p.cpu.register(Register32::Eax), 2);
    assert_eq!(call(&mut p, 0x7000_002c, &[first]), 0);
    fault(
        &mut p,
        ip,
        StopReason::MemoryFault(MemoryError::Unmapped {
            address: u64::from(ip),
        }),
    );
}
