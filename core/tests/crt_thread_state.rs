#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/suspended_thread_executable.rs"]
mod suspended_thread_executable;

use ring3_core::execution::{
    Cpu32, FileContents, FileMetadata, Permissions, Process32, ProcessOptions, ProcessStop,
    Register32, StopReason,
};

const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;
const SECOND: u32 = 0x1102_1000;
const ERRNO: u32 = 0x7000_2020;
const STACK: u32 = 0x1000_ff00;
const PATH: u32 = 0x0040_2300;
const MODE: u32 = PATH + 32;
const OUTPUT: u32 = PATH + 64;

fn load() -> Process32 {
    let mut p = Process32::load_with_options(
        &suspended_thread_executable::pe32(),
        80,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\sample.bin",
                size: 3,
            }],
            file_contents: &[FileContents {
                path: b"C:\\sample.bin",
                bytes: b"abc",
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(u64::from(PATH), b"sample.bin\0").unwrap();
    p.memory.write(u64::from(MODE), b"rb\0").unwrap();
    p
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
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

fn call(p: &mut Process32, offset: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, offset, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + if offset == 0x21c { 8 } else { 4 });
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

fn create(p: &mut Process32) -> u32 {
    call(p, 0x548, &[0, 0, 0x0040_1000, 0, 4, 0])
}

#[test]
fn errno_pointers_are_distinct_stable_and_retained_after_handle_close() {
    let mut p = load();
    assert_eq!(call(&mut p, 0x124, &[]), ERRNO);
    let pages = p.memory.mapped_pages();
    let first = create(&mut p);
    create(&mut p);
    assert_eq!(p.memory.mapped_pages(), pages + 34);
    let actors = [
        (PRIMARY, ERRNO),
        (CHILD, CHILD + 0x40),
        (SECOND, SECOND + 0x40),
    ];
    for (index, &(teb, pointer)) in actors.iter().enumerate() {
        p.cpu.set_fs_base(teb);
        assert_eq!(call(&mut p, 0x124, &[]), pointer);
        assert_eq!(word(&p, pointer), 0);
        put(&mut p, pointer, 70 + u32::try_from(index).unwrap());
        put(&mut p, teb + 0x24, 1);
    }
    assert_eq!(call(&mut p, 0x21c, &[first]), 1);
    for (index, &(teb, pointer)) in actors.iter().enumerate() {
        p.cpu.set_fs_base(teb);
        p.memory
            .protect(u64::from(pointer & !4095), 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(call(&mut p, 0x124, &[]), pointer);
        p.memory
            .protect(u64::from(pointer & !4095), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(word(&p, pointer), 70 + u32::try_from(index).unwrap());
    }
}

#[test]
fn interleaved_random_sequences_reseed_only_the_caller() {
    let mut p = load();
    assert_eq!(call(&mut p, 0x164, &[]), 41);
    let first = create(&mut p);
    create(&mut p);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, 0x164, &[]), 41);
    call(&mut p, 0x160, &[1792]);
    p.cpu.set_fs_base(SECOND);
    assert_eq!(call(&mut p, 0x164, &[]), 41);
    call(&mut p, 0x160, &[0]);
    assert_eq!(call(&mut p, 0x21c, &[first]), 1);
    for (actor, values) in [
        (PRIMARY, [18467, 6334, 26500]),
        (CHILD, [5890, 1279, 19497]),
        (SECOND, [38, 7719, 21238]),
    ] {
        p.cpu.set_fs_base(actor);
        p.memory
            .protect(u64::from(actor), 4096, Permissions::NONE)
            .unwrap();
        for value in values {
            assert_eq!(call(&mut p, 0x164, &[]), value);
        }
    }
}

#[test]
fn caller_errno_faults_retry_without_changing_other_errors_or_outputs() {
    for (offset, args, value, error) in [
        (0x11c, vec![u32::MAX], 0, 12),
        (0x17c, vec![u32::MAX], u32::MAX, 0),
        (0x1a8, vec![0], 0, 22),
        (0x1c0, vec![0], 0, 22),
        (0x1ac, vec![0, MODE], u32::MAX, 22),
        (0x1bc, vec![0, 4, MODE], u32::MAX, 22),
        (0x1b8, vec![0, MODE], u32::MAX, 22),
        (0x15c, vec![MODE, OUTPUT], u32::MAX, 2),
        (0x188, vec![MODE], u32::MAX, 2),
        (0x194, vec![MODE, MODE], 0, 2),
    ] {
        let mut p = load();
        create(&mut p);
        p.cpu.set_fs_base(CHILD);
        put(&mut p, ERRNO, 77);
        put(&mut p, CHILD + 0x40, 88);
        put(&mut p, PRIMARY + 0x34, 55);
        put(&mut p, CHILD + 0x34, 66);
        put(&mut p, OUTPUT, 99);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ)
            .unwrap();
        let before = prepare(&mut p, offset, &args);
        if error != 0 {
            let run = p.run(1);
            assert!(
                matches!(run.reason, ProcessStop::Stopped(StopReason::MemoryFault(_))),
                "api {offset:x}"
            );
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!(word(&p, CHILD + 0x40), 88);
        }
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(call(&mut p, offset, &args), value);
        assert_eq!(word(&p, CHILD + 0x40), if error == 0 { 88 } else { error });
        assert_eq!(word(&p, ERRNO), 77);
        assert_eq!(word(&p, OUTPUT), 99);
        assert_eq!(word(&p, PRIMARY + 0x34), 55);
        assert_eq!(word(&p, CHILD + 0x34), 66);
    }
}

#[test]
fn failed_creation_and_unknown_fs_cannot_publish_or_consume_crt_state() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    prepare(&mut p, 0x548, &[0, 0, 0, 0, 4, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.memory.mapped_pages(), pages);
    for actor in [CHILD, 0, u32::MAX] {
        p.cpu.set_fs_base(actor);
        for (offset, args) in [
            (0x124, vec![]),
            (0x164, vec![]),
            (0x160, vec![1792]),
            (0x1c0, vec![0]),
        ] {
            let before = prepare(&mut p, offset, &args);
            let run = p.run(1);
            assert_eq!(
                run.reason,
                ProcessStop::UnsupportedApi {
                    address: before.eip
                }
            );
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
        assert_eq!(call(&mut p, 0x17c, &[b'A'.into()]), b'a'.into());
    }
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, 0x164, &[]), 41);
    create(&mut p);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, 0x164, &[]), 41);
    let pointer = call(&mut p, 0x124, &[]);
    assert_eq!(word(&p, pointer), 0);
}

#[test]
fn allocation_failures_keep_caller_errno_and_shared_owners_retryable() {
    for offset in [0x134, 0x16c, 0x128, 0x194] {
        let mut p = load();
        create(&mut p);
        p.cpu.set_fs_base(CHILD);
        let table = call(&mut p, 0x11c, &[4096]);
        assert_ne!(table, 0);
        put(&mut p, OUTPUT, table);
        put(&mut p, OUTPUT + 4, table + 4096);
        put(&mut p, table, 123);
        let this = OUTPUT + 16;
        p.memory
            .write(u64::from(this + 8), b".?AVSample@@\0")
            .unwrap();
        p.cpu.set_register(Register32::Ecx, this);
        let fill_size = u32::try_from((80 - p.memory.mapped_pages()) * 4096).unwrap();
        let fill = call(&mut p, 0x11c, &[fill_size]);
        assert_ne!(fill, 0);
        put(&mut p, ERRNO, 77);
        put(&mut p, CHILD + 0x40, 88);
        let args = match offset {
            0x134 => vec![PATH],
            0x16c => vec![],
            0x128 => vec![42, OUTPUT, OUTPUT + 4],
            _ => vec![PATH, MODE],
        };
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ)
            .unwrap();
        let before = prepare(&mut p, offset, &args);
        let run = p.run(1);
        assert!(
            matches!(run.reason, ProcessStop::Stopped(StopReason::MemoryFault(_))),
            "api {offset:x}"
        );
        assert_eq!(p.cpu, before);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.memory.mapped_pages(), 80);
        assert_eq!(word(&p, OUTPUT), table);
        assert_eq!(word(&p, OUTPUT + 4), table + 4096);
        assert_eq!(word(&p, this + 4), 0);
        assert_eq!(word(&p, table), 123);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(call(&mut p, offset, &args), 0);
        assert_eq!(word(&p, CHILD + 0x40), 12);
        assert_eq!(word(&p, ERRNO), 77);
        call(&mut p, 0x120, &[fill]);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::NONE)
            .unwrap();
        assert_ne!(call(&mut p, offset, &args), 0);
        assert_eq!(word(&p, ERRNO), 77);
    }
}

#[test]
fn shared_stream_position_survives_caller_error_faults() {
    let mut p = load();
    create(&mut p);
    let stream = call(&mut p, 0x194, &[PATH, MODE]);
    assert_ne!(stream, 0);
    p.cpu.set_fs_base(CHILD);
    put(&mut p, ERRNO, 77);
    put(&mut p, CHILD + 0x40, 88);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x198, &[OUTPUT, 1, 2, stream]), 2);
    assert_eq!(call(&mut p, 0x1a4, &[stream]), 2);
    for (offset, args) in [(0x1a0, vec![stream, 0, 3]), (0x1a4, vec![stream])] {
        if offset == 0x1a4 {
            assert_eq!(call(&mut p, 0x1a0, &[stream, i32::MAX as u32, 0]), 0);
            assert_eq!(call(&mut p, 0x1a0, &[stream, 1, 1]), 0);
        }
        let before = prepare(&mut p, offset, &args);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(call(&mut p, offset, &args), u32::MAX);
        assert_eq!(word(&p, CHILD + 0x40), 22);
        assert_eq!(word(&p, ERRNO), 77);
        p.cpu.set_fs_base(PRIMARY);
        if offset == 0x1a0 {
            assert_eq!(call(&mut p, 0x1a4, &[stream]), 2);
        } else {
            assert_eq!(call(&mut p, 0x1a0, &[stream, u32::MAX, 1]), 0);
            assert_eq!(call(&mut p, 0x1a4, &[stream]), i32::MAX as u32);
        }
        p.cpu.set_fs_base(CHILD);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::NONE)
            .unwrap();
    }
    assert_eq!(call(&mut p, 0x19c, &[stream]), 0);
}
