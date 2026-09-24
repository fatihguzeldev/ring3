#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/suspended_thread_executable.rs"]
mod suspended_thread_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2300;
const WIDE: u32 = SOURCE + 32;
const OUTPUT: u32 = SOURCE + 64;
const SIZE: u32 = SOURCE + 96;
const USED: u32 = SIZE + 4;

fn load() -> Process32 {
    let mut p = Process32::load_with_options(
        &suspended_thread_executable::pe32(),
        64,
        ProcessOptions {
            environment: &[b"demo=value"],
            directories: &[b"C:\\demo"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p.memory.write(u64::from(SOURCE), b"absent\0").unwrap();
    p.memory.write(u64::from(WIDE), b"a\0b\0\0\0").unwrap();
    p.memory.write(u64::from(OUTPUT), &[0x55; 16]).unwrap();
    put(&mut p, OUTPUT, 0);
    put(&mut p, PRIMARY + 0x34, 77);
    put(&mut p, CHILD + 0x34, 88);
    put(&mut p, USED, 99);
    p
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn bytes(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut output = vec![0; size];
    p.memory.read(u64::from(address), &mut output).unwrap();
    output
}

fn prepare(p: &mut Process32, offset: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = 0x7000_0000 + offset;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
        put(p, STACK + u32::try_from(index).unwrap() * 4, *value);
    }
    p.cpu
}

fn call(p: &mut Process32, offset: u32, args: &[u32], expected: u32) {
    let mut cpu = prepare(p, offset, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    cpu.eip = 0x0040_1000;
    cpu.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    cpu.set_register(Register32::Eax, expected);
    assert_eq!(p.cpu, cpu);
}

fn failures() -> Vec<(u32, Vec<u32>, u32)> {
    vec![
        (0x240, vec![SOURCE, OUTPUT, 16], 203),
        (0x20c, vec![OUTPUT, SIZE], 111),
        (0x90, vec![1252, 0], 87),
        (0x33c, vec![1252, 0, 0, 1, OUTPUT, 4], 87),
        (0x33c, vec![1252, 0, SOURCE, 2, OUTPUT, 1], 122),
        (0x338, vec![1252, 0, 0, 1, OUTPUT, 4, 0, USED], 87),
        (0x338, vec![1252, 0, WIDE, 2, OUTPUT, 1, 0, USED], 122),
        (0xe4, vec![OUTPUT, 0x5000_0000, 4], 87),
        (0xf0, vec![OUTPUT, 0x5000_0000], 87),
        (0xf4, vec![OUTPUT, 0x5000_0000], 87),
    ]
}

#[test]
fn basic_provider_errors_target_only_the_calling_teb() {
    for actor in [PRIMARY, CHILD] {
        for (offset, args, error) in failures() {
            let mut p = load();
            p.cpu.set_fs_base(actor);
            let output = bytes(&p, OUTPUT, 16);
            call(&mut p, offset, &args, 0);
            assert_eq!(
                p.last_error().unwrap(),
                error,
                "api {offset:x}, actor {actor:x}"
            );
            let (other, sentinel) = if actor == PRIMARY {
                (CHILD, 88_u32)
            } else {
                (PRIMARY, 77)
            };
            assert_eq!(bytes(&p, other + 0x34, 4), sentinel.to_le_bytes());
            assert_eq!(bytes(&p, OUTPUT, 16), output);
            assert_eq!(bytes(&p, USED, 4), 99_u32.to_le_bytes());
            assert_eq!(
                bytes(&p, SIZE, 4),
                if offset == 0x20c { 6_u32 } else { 0 }.to_le_bytes()
            );
        }
    }
}

#[test]
fn caller_error_faults_preserve_cpu_outputs_and_primary_error() {
    for actor in [CHILD, 0x5000_0000, u32::MAX - 20] {
        for (offset, args, _) in failures() {
            let mut p = load();
            p.cpu.set_fs_base(actor);
            p.memory
                .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
                .unwrap();
            let output = bytes(&p, OUTPUT, 16);
            let before = prepare(&mut p, offset, &args);
            let result = p.run(1);
            assert!(
                matches!(
                    result.reason,
                    ProcessStop::Stopped(StopReason::MemoryFault(_))
                ),
                "api {offset:x}"
            );
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!(bytes(&p, OUTPUT, 16), output);
            assert_eq!(bytes(&p, SIZE, 8), [0, 0, 0, 0, 99, 0, 0, 0]);
            assert_eq!(bytes(&p, PRIMARY + 0x34, 4), 77_u32.to_le_bytes());
            assert_eq!(bytes(&p, CHILD + 0x34, 4), 88_u32.to_le_bytes());
        }
    }
}

#[test]
fn successful_basic_calls_do_not_require_access_to_the_teb() {
    let cases = [
        (0x240, vec![SOURCE, OUTPUT, 16], 5),
        (0x20c, vec![OUTPUT, SIZE], 1),
        (0x90, vec![1252, OUTPUT], 1),
        (0x33c, vec![1252, 0, SOURCE, 2, OUTPUT, 4], 2),
        (0x338, vec![1252, 0, WIDE, 2, OUTPUT, 4, 0, USED], 2),
        (0xe4, vec![OUTPUT, SOURCE, 4], OUTPUT),
        (0xf0, vec![OUTPUT, SOURCE], OUTPUT),
        (0xf4, vec![OUTPUT, SOURCE], OUTPUT),
    ];
    for actor in [CHILD, 0x5000_0000, u32::MAX - 20] {
        for (offset, args, result) in &cases {
            let mut p = load();
            p.memory.write(u64::from(SOURCE), b"demo\0").unwrap();
            put(&mut p, SIZE, 6);
            p.memory
                .protect(u64::from(CHILD), PAGE_SIZE, Permissions::NONE)
                .unwrap();
            p.cpu.set_fs_base(actor);
            call(&mut p, *offset, args, *result);
            p.memory
                .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
                .unwrap();
            assert_eq!(bytes(&p, PRIMARY + 0x34, 4), 77_u32.to_le_bytes());
            assert_eq!(bytes(&p, CHILD + 0x34, 4), 88_u32.to_le_bytes());
        }
    }
}

#[test]
fn computer_name_size_aliases_keep_caller_error_preflight_and_write_order() {
    let mut p = load();
    p.cpu.set_fs_base(CHILD);
    put(&mut p, CHILD + 0x34, 0);
    call(&mut p, 0x20c, &[OUTPUT, CHILD + 0x34], 0);
    assert_eq!(p.last_error().unwrap(), 111);
    assert_eq!(bytes(&p, PRIMARY + 0x34, 4), 77_u32.to_le_bytes());
    put(&mut p, PRIMARY + 0x34, 0);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, 0x20c, &[OUTPUT, PRIMARY + 0x34]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, PRIMARY + 0x34, 4), [0; 4]);
    assert_eq!(p.last_error().unwrap(), 111);
}

#[test]
fn heap_and_directory_failures_target_caller_error_without_partial_state() {
    let bad = 0x5000_0000;
    let cases = [
        (0x28, vec![0, u32::MAX], 0, 8),
        (0x78, vec![0, u32::MAX], 0, 8),
        (0x2c, vec![bad], bad, 6),
        (0x84, vec![bad], bad, 6),
        (0xd4, vec![bad, 8, 2], 0, 6),
        (0x7c, vec![bad], 0, 6),
        (0x80, vec![bad], 0, 6),
        (0x230, vec![SOURCE], 0, 3),
        (0x290, vec![SOURCE], u32::MAX, 2),
        (0x294, vec![SOURCE, OUTPUT, 16], 0, 2),
        (0x234, vec![SOURCE, OUTPUT], u32::MAX, 2),
        (0x238, vec![bad, OUTPUT], 0, 6),
        (0x23c, vec![bad], 0, 6),
    ];
    for readonly in [false, true] {
        for (offset, args, result, error) in &cases {
            let mut p = load();
            p.cpu.set_fs_base(CHILD);
            let pages = p.memory.mapped_pages();
            let output = bytes(&p, OUTPUT, 320);
            if readonly {
                p.memory
                    .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
                    .unwrap();
                let cpu = prepare(&mut p, *offset, args);
                let run = p.run(1);
                assert!(
                    matches!(run.reason, ProcessStop::Stopped(StopReason::MemoryFault(_))),
                    "api {offset:x}"
                );
                assert_eq!((run.instructions, run.api_calls), (0, 0));
                assert_eq!(p.cpu, cpu);
                assert_eq!(p.last_error().unwrap(), 88);
                p.memory
                    .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
            }
            assert_eq!(p.memory.mapped_pages(), pages);
            assert_eq!(bytes(&p, OUTPUT, 320), output);
            call(&mut p, *offset, args, *result);
            assert_eq!(p.last_error().unwrap(), *error, "api {offset:x}");
            assert_eq!(p.memory.mapped_pages(), pages);
            assert_eq!(bytes(&p, OUTPUT, 320), output);
            assert_eq!(bytes(&p, PRIMARY + 0x34, 4), 77_u32.to_le_bytes());
            call(&mut p, 0x228, &[16, OUTPUT], 3);
            assert_eq!(bytes(&p, OUTPUT, 4), b"C:\\\0");
        }
    }
}

#[test]
fn final_global_unlock_fault_preserves_lock_count_for_retry() {
    let mut p = load();
    call(&mut p, 0x78, &[2, 16], 0x2000_0002);
    call(&mut p, 0x7c, &[0x2000_0002], 0x2000_0000);
    p.cpu.set_fs_base(CHILD);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
        .unwrap();
    let cpu = prepare(&mut p, 0x80, &[0x2000_0002]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.last_error().unwrap(), 88);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, 0x80, &[0x2000_0002], 0);
    assert_eq!(p.last_error().unwrap(), 0);
    call(&mut p, 0x80, &[0x2000_0002], 0);
    assert_eq!(p.last_error().unwrap(), 158);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::NONE)
        .unwrap();
    call(&mut p, 0x84, &[0x2000_0002], 0);
    assert_eq!(bytes(&p, PRIMARY + 0x34, 4), 77_u32.to_le_bytes());
}

#[test]
fn searches_remain_shared_and_error_faults_do_not_consume_capacity_or_identity() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"*\0").unwrap();
    for index in 0..64 {
        call(&mut p, 0x234, &[SOURCE, OUTPUT], 0x7300_0004 + index * 4);
    }
    let output = bytes(&p, OUTPUT, 320);
    p.cpu.set_fs_base(CHILD);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
        .unwrap();
    for (offset, args) in [(0x234, [SOURCE, OUTPUT]), (0x238, [0x7300_0004, OUTPUT])] {
        let cpu = prepare(&mut p, offset, &args);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, cpu);
        assert_eq!(bytes(&p, OUTPUT, 320), output);
    }
    call(&mut p, 0x23c, &[0x7300_0004], 1);
    call(&mut p, 0x234, &[SOURCE, OUTPUT], 0x7300_0104);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, 0x234, &[SOURCE, OUTPUT], u32::MAX);
    assert_eq!(p.last_error().unwrap(), 8);
    call(&mut p, 0x238, &[0x7300_0104, OUTPUT], 0);
    assert_eq!(p.last_error().unwrap(), 18);
    p.memory.write(u64::from(SOURCE), b"demo\0").unwrap();
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::NONE)
        .unwrap();
    call(&mut p, 0x230, &[SOURCE], 1);
    p.cpu.set_fs_base(PRIMARY);
    call(&mut p, 0x228, &[16, OUTPUT], 7);
    assert_eq!(bytes(&p, OUTPUT, 8), b"C:\\demo\0");
    assert_eq!(p.last_error().unwrap(), 77);
}
