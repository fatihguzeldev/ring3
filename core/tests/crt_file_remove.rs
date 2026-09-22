#[path = "support/file_remove_executable.rs"]
mod file_remove_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, FileMetadata, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const REMOVE: u32 = 0x7000_0188;
const STAT: u32 = 0x7000_015c;
const FIRST: u32 = 0x7000_0234;
const NEXT: u32 = 0x7000_0238;
const CLOSE: u32 = 0x7000_023c;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x0040_2600;
const STACK: u32 = 0x1000_ff00;
const ERRNO: u32 = 0x7000_2020;

fn load() -> Process32 {
    Process32::load_with_options(
        &file_remove_executable::pe32(),
        64,
        ProcessOptions {
            files: &[
                FileMetadata {
                    path: b"C:\\folder\\erase.tmp",
                    size: 42,
                },
                FileMetadata {
                    path: b"C:\\folder\\keep.tmp",
                    size: 7,
                },
                FileMetadata {
                    path: b"C:\\lonely\\last.bin",
                    size: 3,
                },
            ],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}
fn write(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}
fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn path(p: &mut Process32, value: &[u8]) {
    p.memory.write(u64::from(SOURCE), value).unwrap();
}
fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Esp, STACK);
    write(p, STACK, 0x0040_1000);
    for (i, value) in args.iter().enumerate() {
        write(p, STACK + 4 + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu
}
fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut cpu = prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = p.cpu.register(Register32::Eax);
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, result);
    let cleanup = if matches!(api, REMOVE | STAT) {
        4
    } else {
        u32::try_from(args.len() + 1).unwrap() * 4
    };
    cpu.set_register(Register32::Esp, STACK + cleanup);
    assert_eq!(p.cpu, cpu);
    result
}
fn found_name(p: &Process32) -> String {
    let mut bytes = [0; 260];
    p.memory.read(u64::from(OUTPUT + 44), &mut bytes).unwrap();
    String::from_utf8(bytes.into_iter().take_while(|b| *b != 0).collect()).unwrap()
}

#[test]
fn imported_remove_mutates_only_existing_guest_files_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = load();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (4, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(call(&mut p, STAT, &[0x0040_2180, OUTPUT]), u32::MAX);
        assert_eq!(word(&p, ERRNO), 2);
    }
}

#[test]
fn removed_leaves_disappear_but_parents_and_existing_search_snapshots_survive() {
    let mut p = load();
    path(&mut p, b"folder\\*\0");
    let handle = call(&mut p, FIRST, &[SOURCE, OUTPUT]);
    assert_eq!(found_name(&p), "erase.tmp");
    path(&mut p, b"folder\\keep.tmp\0");
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
    assert_eq!(call(&mut p, NEXT, &[handle, OUTPUT]), 1);
    assert_eq!(found_name(&p), "keep.tmp");
    assert_eq!(word(&p, OUTPUT + 32), 7);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    path(&mut p, b"folder\\*\0");
    let handle = call(&mut p, FIRST, &[SOURCE, OUTPUT]);
    assert_eq!(found_name(&p), "erase.tmp");
    assert_eq!(call(&mut p, NEXT, &[handle, OUTPUT]), 0);
    assert_eq!(p.last_error().unwrap(), 18);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    path(&mut p, b"folder\\erase.tmp\0");
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
    path(&mut p, b"folder\\*\0");
    assert_eq!(call(&mut p, FIRST, &[SOURCE, OUTPUT]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 2);
    path(&mut p, b"folder\0");
    assert_eq!(call(&mut p, STAT, &[SOURCE, OUTPUT]), 0);
    assert_eq!(word(&p, OUTPUT + 4) >> 16, 0x41ff);
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), u32::MAX);
    assert_eq!(word(&p, ERRNO), 13);
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 1);
    path(&mut p, b".\0");
    assert_eq!(call(&mut p, STAT, &[SOURCE, OUTPUT]), 0);
    path(&mut p, b"C:\\*\0");
    let handle = call(&mut p, FIRST, &[SOURCE, OUTPUT]);
    assert_eq!(found_name(&p), "folder");
    assert_eq!(word(&p, OUTPUT), 0x10);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
}

#[test]
fn normalized_names_errors_and_deletions_are_process_local() {
    for name in [
        b"c:\\FOLDER\\ERASE.tmp\0".as_slice(),
        b"\\folder\\erase.tmp\0",
        b"folder\\..\\folder\\erase.tmp\0",
    ] {
        let mut p = load();
        let mut other = load();
        write(&mut p, ERRNO, 77);
        path(&mut p, name);
        assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
        assert_eq!(word(&p, ERRNO), 77);
        assert_eq!(call(&mut p, REMOVE, &[SOURCE]), u32::MAX);
        assert_eq!(word(&p, ERRNO), 2);
        path(&mut other, name);
        assert_eq!(call(&mut other, STAT, &[SOURCE, OUTPUT]), 0);
    }
    let mut p = load();
    for (name, error) in [
        (b"missing\0".as_slice(), 2),
        (b"\0", 2),
        (b"folder\\erase.tmp\\\0", 2),
        (b".\0", 13),
        (b"C:\\\0", 13),
    ] {
        path(&mut p, name);
        assert_eq!(call(&mut p, REMOVE, &[SOURCE]), u32::MAX);
        assert_eq!(word(&p, ERRNO), error);
    }
    path(&mut p, b"folder\\erase.tmp\0");
    assert_eq!(call(&mut p, STAT, &[SOURCE, OUTPUT]), 0);
    path(&mut p, b"folder\0");
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 1);
    path(&mut p, b"erase.tmp\0");
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
}

#[test]
fn failures_are_atomic_and_success_does_not_require_writable_error_cells() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    write(&mut p, ERRNO, 77);
    write(&mut p, 0x7ffd_e034, 88);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    for name in [b"missing\0".as_slice(), b"folder\0"] {
        path(&mut p, name);
        let cpu = prepare(&mut p, REMOVE, &[SOURCE]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, cpu);
        assert_eq!(word(&p, ERRNO), 77);
    }
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    path(&mut p, b"folder\\erase.tmp\0");
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(word(&p, ERRNO), 77);
    assert_eq!(p.last_error().unwrap(), 88);
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), u32::MAX);
    assert_eq!(word(&p, ERRNO), 2);
    assert_eq!(p.last_error().unwrap(), 88);
}

#[test]
fn unsupported_or_unreadable_paths_and_frames_do_not_delete_files() {
    let mut p = load();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_ffff, b"x").unwrap();
    p.memory.write(0x0040_2fff, b"x").unwrap();
    for source in [0, 0xffff_ffff, 0x0040_2fff] {
        let cpu = prepare(&mut p, REMOVE, &[source]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, cpu);
    }
    for name in [
        b"C:erase.tmp\0".as_slice(),
        b"\\\\server\\file\0",
        b"NUL\0",
        b"folder/erase.tmp\0",
        b"\x80\0",
        b"file.\0",
    ] {
        path(&mut p, name);
        let cpu = prepare(&mut p, REMOVE, &[SOURCE]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: REMOVE }
        );
        assert_eq!(p.cpu, cpu);
    }
    path(&mut p, b"folder\\erase.tmp\0");
    let cpu = prepare(&mut p, REMOVE, &[SOURCE]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let cpu = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, cpu);
    assert_eq!(call(&mut p, STAT, &[SOURCE, OUTPUT]), 0);
    assert_eq!(call(&mut p, REMOVE, &[SOURCE]), 0);
}

#[test]
fn terminated_paths_obey_the_32k_bound_and_address_limit() {
    let mut p = load();
    p.memory
        .map_zeroed(0x5000_0000, 32768, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x5000_0000, &vec![b'a'; 32768]).unwrap();
    let cpu = prepare(&mut p, REMOVE, &[0x5000_0000]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: REMOVE }
    );
    assert_eq!(p.cpu, cpu);
    let mut name = vec![b'\\'; 32751];
    name[0] = b'.';
    name.extend_from_slice(b"folder\\erase.tmp\0");
    assert_eq!(name.len(), 32768);
    p.memory.write(0x5000_0000, &name).unwrap();
    assert_eq!(call(&mut p, REMOVE, &[0x5000_0000]), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, b"x\0").unwrap();
    assert_eq!(call(&mut p, REMOVE, &[0xffff_fffe]), u32::MAX);
    assert_eq!(word(&p, ERRNO), 2);
}
