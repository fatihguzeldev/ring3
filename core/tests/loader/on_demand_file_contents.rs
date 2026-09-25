use super::imported_executable;
use super::on_demand_file_cases;

#[test]
fn imported_opens_resume_read_and_refill_evicted_snapshots() {
    on_demand_file_cases::imported_opens_resume_read_and_refill_evicted_snapshots();
}

use ring3_core::execution::{
    FileContents, FileContentsMode, FileMetadata, LoadError, Permissions, Process32,
    ProcessOptions, ProcessStop, Register32, StopReason, SupplyFileContentsError,
};

const PATH: u32 = 0x0040_2200;
const STACK: u32 = 0x1000_ff00;
const OPEN: u32 = 0x7000_05b4;
const CLOSE: u32 = 0x7000_021c;
const READ: u32 = 0x7000_05c4;
const OUTPUT: u32 = 0x0040_2400;
const COUNT: u32 = 0x0040_2500;
const ERROR: u32 = 0x7ffd_e034;
const ERRNO: u32 = 0x7000_2020;

fn load(
    mode: FileContentsMode,
    files: &[FileMetadata<'_>],
    contents: &[FileContents<'_>],
) -> Result<Process32, LoadError> {
    Process32::load_with_options(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["CreateFileA"]),
        1024,
        ProcessOptions {
            files,
            file_contents: contents,
            file_contents_mode: mode,
            ..ProcessOptions::default()
        },
    )
}

fn process(limit: usize, supplied: &[FileContents<'_>]) -> Process32 {
    let files: Vec<_> = [b"C:\\a", b"C:\\b", b"C:\\c"]
        .iter()
        .map(|&path| FileMetadata { path, size: 4 })
        .collect();
    load(
        FileContentsMode::OnDemand { cache_bytes: limit },
        &files,
        supplied,
    )
    .unwrap()
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

fn open_frame(p: &mut Process32, path: &[u8]) {
    p.memory.write(u64::from(PATH), path).unwrap();
    put(p, PATH + u32::try_from(path.len()).unwrap(), 0);
    prepare(p, OPEN, &[PATH, 0x8000_0000, 1, 0, 3, 0, 0]);
}

fn finish(p: &mut Process32) -> u32 {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    p.cpu.register(Register32::Eax)
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    finish(p)
}

fn request(p: &mut Process32, path: &[u8]) -> u64 {
    open_frame(p, path);
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::FileContentsRequired);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    let pending = p.pending_file_contents().unwrap();
    assert_eq!(pending.path, path);
    pending.id
}

#[test]
fn positive_runs_pause_before_any_guest_work_and_bad_supplies_preserve_the_request() {
    let mut p = process(4, &[]);
    put(&mut p, ERROR, 77);
    put(&mut p, ERRNO, 88);
    let id = request(&mut p, b"C:\\a");
    let before = p.cpu;
    assert_eq!(
        p.run(0).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    for budget in [1, 4096, u64::MAX] {
        let run = p.run(budget);
        assert_eq!(
            (run.reason, run.instructions, run.api_calls),
            (ProcessStop::FileContentsRequired, 0, 0)
        );
    }
    assert_eq!(
        p.supply_file_contents(id + 1, Box::from(*b"abcd")),
        Err(SupplyFileContentsError::StaleRequest)
    );
    assert_eq!(
        p.supply_file_contents(id, Box::from(*b"abc")),
        Err(SupplyFileContentsError::SizeMismatch)
    );
    assert_eq!(p.cpu, before);
    assert_eq!((word(&p, ERROR), word(&p, ERRNO)), (77, 88));
    assert_eq!(p.pending_file_contents().unwrap().id, id);
    p.supply_file_contents(id, Box::from(*b"abcd")).unwrap();
    assert_eq!(
        p.supply_file_contents(id, Box::from(*b"abcd")),
        Err(SupplyFileContentsError::NoPendingRequest)
    );
    assert_eq!(finish(&mut p), 0x7a00_0004);
    assert_eq!(call(&mut p, READ, &[0x7a00_0004, OUTPUT, 4, COUNT, 0]), 1);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"abcd"));
}

#[test]
fn win32_and_crt_pins_share_one_resident_file_and_prevent_eviction_until_last_close() {
    let mut p = process(
        8,
        &[FileContents {
            path: b"C:\\a",
            bytes: b"aaaa",
        }],
    );
    open_frame(&mut p, b"C:\\a");
    let handle = finish(&mut p);
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    let stream = call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]);
    assert_ne!(stream, 0);
    let id = request(&mut p, b"C:\\b");
    p.supply_file_contents(id, Box::from(*b"bbbb")).unwrap();
    let other = finish(&mut p);
    open_frame(&mut p, b"C:\\c");
    assert_eq!(finish(&mut p), u32::MAX);
    assert_eq!(word(&p, ERROR), 8);
    assert!(p.pending_file_contents().is_none());
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    open_frame(&mut p, b"C:\\c");
    assert_eq!(finish(&mut p), u32::MAX);
    assert_eq!(call(&mut p, 0x7000_0198, &[OUTPUT, 1, 4, stream]), 4);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"aaaa"));
    assert_eq!(call(&mut p, 0x7000_019c, &[stream]), 0);
    let next = request(&mut p, b"C:\\c");
    assert!(next > id);
    p.supply_file_contents(next, Box::from(*b"cccc")).unwrap();
    let third = finish(&mut p);
    assert_eq!(call(&mut p, READ, &[other, OUTPUT, 4, COUNT, 0]), 1);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"bbbb"));
    assert_eq!(call(&mut p, CLOSE, &[other]), 1);
    assert_eq!(call(&mut p, CLOSE, &[third]), 1);
    let again = request(&mut p, b"C:\\a");
    assert!(again > next);
    assert_eq!(
        p.supply_file_contents(id, Box::from(*b"aaaa")),
        Err(SupplyFileContentsError::StaleRequest)
    );
    p.supply_file_contents(again, Box::from(*b"aaaa")).unwrap();
    let reopened = finish(&mut p);
    assert_eq!(call(&mut p, READ, &[reopened, OUTPUT, 4, COUNT, 0]), 1);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"aaaa"));
}

#[test]
fn load_budgets_and_default_metadata_only_behavior_remain_explicit() {
    let files = [FileMetadata {
        path: b"C:\\a",
        size: 4,
    }];
    for limit in [1024 * 1024 * 1024 + 1, usize::MAX] {
        assert!(matches!(
            load(FileContentsMode::OnDemand { cache_bytes: limit }, &[], &[]),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
    assert!(matches!(
        load(
            FileContentsMode::OnDemand { cache_bytes: 3 },
            &files,
            &[FileContents {
                path: b"C:\\a",
                bytes: b"aaaa"
            }]
        ),
        Err(LoadError::FileContentsLimitExceeded)
    ));
    let mut p = load(FileContentsMode::Supplied, &files, &[]).unwrap();
    open_frame(&mut p, b"C:\\a");
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: OPEN }
    );
    assert!(p.pending_file_contents().is_none());
    let mut p = process(3, &[]);
    open_frame(&mut p, b"C:\\a");
    let before = p.cpu;
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert!(p.pending_file_contents().is_none());
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(finish(&mut p), u32::MAX);
    assert_eq!(word(&p, ERROR), 8);
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]), 0);
    assert_eq!(word(&p, ERRNO), 12);
    assert!(p.pending_file_contents().is_none());
}

#[test]
fn a_scheduled_child_keeps_its_context_while_the_whole_process_waits_for_contents() {
    let marker = 0x0040_2600_u32;
    let mut code = vec![0xff, 0x05];
    code.extend(marker.to_le_bytes());
    code.extend([0xeb, 0xf8]);
    code.resize(256, 0xcc);
    for value in [0_u32, 0, 3, 0, 1, 0x8000_0000, PATH] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6]);
    for value in [0, COUNT, 4, OUTPUT] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x56, 0xff, 0x15, 0x64, 0x20, 0x40, 0]);
    code.extend([0x56, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc]);
    let exe = imported_executable::pe32(
        &code,
        "kernel32.dll",
        &["CreateFileA", "ReadFile", "CloseHandle"],
    );
    let mut p = Process32::load_with_options(
        &exe,
        96,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\a",
                size: 4,
            }],
            file_contents_mode: FileContentsMode::OnDemand { cache_bytes: 4 },
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(u64::from(PATH), b"C:\\a\0").unwrap();
    assert_eq!(
        p.run(4).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    let child = call(&mut p, 0x7000_0548, &[0, 0, 0x0040_1100, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    let run = p.run(10000);
    assert_eq!(run.reason, ProcessStop::FileContentsRequired);
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    assert!(word(&p, marker) > 0);
    let before = p.cpu;
    let progress = word(&p, marker);
    let id = p.pending_file_contents().unwrap().id;
    for budget in [0, 1, 4096, 10000] {
        let waiting = p.run(budget);
        assert_eq!((waiting.instructions, waiting.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, marker), progress);
    }
    p.supply_file_contents(id, Box::from(*b"file")).unwrap();
    assert_eq!(p.cpu, before);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"file"));
    assert_eq!(word(&p, marker), progress);
}

#[test]
fn empty_crt_files_can_be_supplied_with_zero_budget_and_removed_after_close() {
    let mut p = load(
        FileContentsMode::OnDemand { cache_bytes: 0 },
        &[FileMetadata {
            path: b"C:\\empty",
            size: 0,
        }],
        &[],
    )
    .unwrap();
    p.memory.write(u64::from(PATH), b"C:\\empty\0").unwrap();
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    prepare(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]);
    let before = p.cpu;
    assert_eq!(p.run(1).reason, ProcessStop::FileContentsRequired);
    assert_eq!(p.cpu, before);
    let id = p.pending_file_contents().unwrap().id;
    p.supply_file_contents(id, Box::from([])).unwrap();
    let stream = finish(&mut p);
    assert_ne!(stream, 0);
    assert_eq!(call(&mut p, 0x7000_0198, &[OUTPUT, 1, 4, stream]), 0);
    assert_eq!(word(&p, stream + 12), 0x15);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(call(&mut p, 0x7000_019c, &[stream]), 0);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), 0);
    assert_eq!(call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]), 0);
    assert_eq!(word(&p, ERRNO), 2);
    assert!(p.pending_file_contents().is_none());
}

#[test]
fn crt_descriptor_exhaustion_precedes_a_missing_content_request() {
    let mut p = process(
        8,
        &[FileContents {
            path: b"C:\\a",
            bytes: b"aaaa",
        }],
    );
    p.memory.write(u64::from(PATH), b"C:\\a\0").unwrap();
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    let streams: Vec<_> = (0..512)
        .map(|_| call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]))
        .collect();
    assert!(streams.iter().all(|&stream| stream != 0));
    p.memory.write(u64::from(PATH), b"C:\\b\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]), 0);
    assert_eq!(word(&p, ERRNO), 24);
    assert!(p.pending_file_contents().is_none());
    assert_eq!(call(&mut p, 0x7000_019c, &[streams[0]]), 0);
    prepare(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]);
    assert_eq!(p.run(1).reason, ProcessStop::FileContentsRequired);
    let id = p.pending_file_contents().unwrap().id;
    assert_eq!(id, 1);
    p.supply_file_contents(id, Box::from(*b"bbbb")).unwrap();
    let stream = finish(&mut p);
    assert_ne!(stream, 0);
    assert_eq!(call(&mut p, 0x7000_0198, &[OUTPUT, 1, 4, stream]), 4);
    assert_eq!(word(&p, OUTPUT), u32::from_le_bytes(*b"bbbb"));
}

#[test]
fn malformed_open_frames_and_resource_error_faults_do_not_issue_requests() {
    let mut p = process(8, &[]);
    open_frame(&mut p, b"C:\\a");
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert!(p.pending_file_contents().is_none());
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let frame: Vec<_> = [0x0040_1000_u32, PATH, 0x8000_0000, 1, 0, 3, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(0xffff_ffe0, &frame).unwrap();
    p.cpu.set_register(Register32::Esp, 0xffff_ffe0);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert!(p.pending_file_contents().is_none());
    let mut p = process(3, &[]);
    p.memory.write(u64::from(PATH), b"C:\\a\0").unwrap();
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert!(p.pending_file_contents().is_none());
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(finish(&mut p), 0);
    assert_eq!(word(&p, ERRNO), 12);
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    for (index, word) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
}

#[test]
fn missing_contents_pause_without_guest_mutation_and_resume_after_owned_supply() {
    let exe = imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["CreateFileA"]);
    let mut p = Process32::load_with_options(
        &exe,
        64,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\sample",
                size: 4,
            }],
            file_contents_mode: FileContentsMode::OnDemand { cache_bytes: 8 },
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(u64::from(PATH), b"C:\\sample\0").unwrap();
    prepare(&mut p, OPEN, &[PATH, 0x8000_0000, 1, 0, 3, 0, 0]);
    let before = p.cpu;
    let pages = p.memory.mapped_pages();
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::FileContentsRequired);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    let request = p.pending_file_contents().unwrap();
    assert_eq!(request.path, b"C:\\sample");
    assert_eq!(request.size, 4);
    let id = request.id;
    assert_eq!(id, 1);
    p.supply_file_contents(id, Box::from(*b"data")).unwrap();
    assert!(p.pending_file_contents().is_none());
    assert_eq!(p.cpu, before);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.register(Register32::Eax), 0x7a00_0004);
}
