use super::file_stream_cases;

use ring3_core::execution::{
    FileContents, FileMetadata, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const OPEN: u32 = 0x7000_0194;
const READ: u32 = 0x7000_0198;
const CLOSE: u32 = 0x7000_019c;
const SEEK: u32 = 0x7000_01a0;
const TELL: u32 = 0x7000_01a4;
const STACK: u32 = 0x1000_ff00;
const PATH: u32 = 0x0040_2180;
const MODE: u32 = 0x0040_2190;
const OUTPUT: u32 = 0x0040_2400;
const ERRNO: u32 = 0x7000_2020;

fn bytes(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut data = vec![0; length];
    p.memory.read(u64::from(address), &mut data).unwrap();
    data
}

fn word(p: &Process32, address: u32) -> u32 {
    u32::from_le_bytes(bytes(p, address, 4).try_into().unwrap())
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    let frame: Vec<_> = [0x0040_1000]
        .iter()
        .chain(args)
        .flat_map(|v| v.to_le_bytes())
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
}

fn success(p: &mut Process32) -> u32 {
    let mut expected = p.cpu;
    let stack = p.cpu.register(Register32::Esp);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = word(p, stack);
    expected.set_register(Register32::Esp, stack + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    success(p)
}

fn failure(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let result = p.run(1);
    if unsupported {
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: cpu.eip }
        );
    } else {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
}

fn open(p: &mut Process32) -> u32 {
    let pointer = call(p, OPEN, &[PATH, MODE]);
    assert_ne!(pointer, 0);
    pointer
}

#[test]
fn imported_streams_read_supplied_bytes_and_release_the_owned_record() {
    file_stream_cases::verify();
}

#[test]
fn independent_cursors_partial_items_and_eof_have_real_stream_lifetimes() {
    let mut p = file_stream_cases::process(128);
    let a = open(&mut p);
    let b = open(&mut p);
    assert_eq!(
        (word(&p, a + 12), word(&p, a + 16), word(&p, b + 16)),
        (5, 3, 4)
    );
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 11, a]), 11);
    assert_eq!(bytes(&p, OUTPUT, 11), file_stream_cases::PAYLOAD);
    assert_eq!(word(&p, a + 12), 5);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, a]), 0);
    assert_eq!(word(&p, a + 12), 0x15);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 2, 6, b]), 5);
    assert_eq!(bytes(&p, OUTPUT, 11), file_stream_cases::PAYLOAD);
    assert_eq!(word(&p, b + 12), 0x15);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, b]), 0);
    for args in [[u32::MAX, 0, u32::MAX, u32::MAX], [0, u32::MAX, 0, 0]] {
        assert_eq!(call(&mut p, READ, &args), 0);
    }
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(word(&p, ERRNO), 13);
    assert_eq!(call(&mut p, CLOSE, &[a]), 0);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(call(&mut p, CLOSE, &[b]), 0);
    prepare(&mut p, CLOSE, &[b]);
    failure(&mut p, true);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), 0);
    assert_eq!(call(&mut p, OPEN, &[PATH, MODE]), 0);
    assert_eq!(word(&p, ERRNO), 2);
}

#[test]
fn seek_from_end_moves_the_cursor_and_clears_eof() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 12, pointer]), 11);
    assert_eq!(word(&p, pointer + 12), 0x15);
    p.memory
        .write(u64::from(ERRNO), &123_u32.to_le_bytes())
        .unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    assert_eq!(
        call(&mut p, SEEK, &[pointer, (-3_i32).cast_unsigned(), 2]),
        0
    );
    assert_eq!(word(&p, pointer + 12), 5);
    assert_eq!(word(&p, ERRNO), 123);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 3, pointer]), 3);
    assert_eq!(bytes(&p, OUTPUT, 3), b"END");
}

#[test]
fn seek_supports_all_origins_and_positions_beyond_eof() {
    let mut p = file_stream_cases::process(128);
    let a = open(&mut p);
    let b = open(&mut p);
    assert_eq!(call(&mut p, SEEK, &[a, 5, 0]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 2, a]), 2);
    assert_eq!(bytes(&p, OUTPUT, 2), b"\x1az");
    assert_eq!(call(&mut p, SEEK, &[a, (-1_i32).cast_unsigned(), 1]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, a]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"z");
    assert_eq!(call(&mut p, SEEK, &[b, (-1_i32).cast_unsigned(), 2]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, b]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"D");

    assert_eq!(call(&mut p, SEEK, &[a, 4, 2]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, a]), 0);
    assert_eq!(word(&p, a + 12), 0x15);
    assert_eq!(call(&mut p, SEEK, &[a, 0, 2]), 0);
    assert_eq!(word(&p, a + 12), 5);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, a]), 0);
    assert_eq!(word(&p, a + 12), 0x15);

    for args in [
        [a, 0, 3],
        [a, (-12_i32).cast_unsigned(), 2],
        [a, i32::MIN.cast_unsigned(), 1],
    ] {
        assert_eq!(call(&mut p, SEEK, &args), u32::MAX);
        assert_eq!(word(&p, ERRNO), 22);
        assert_eq!(word(&p, a + 12), 0x15);
    }
    assert_eq!(call(&mut p, SEEK, &[a, i32::MAX.cast_unsigned(), 0]), 0);
    assert_eq!(call(&mut p, SEEK, &[a, i32::MAX.cast_unsigned(), 1]), 0);
    assert_eq!(call(&mut p, SEEK, &[a, 2, 1]), u32::MAX);
    assert_eq!(word(&p, ERRNO), 22);
    assert_eq!(call(&mut p, SEEK, &[a, 0, 0]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, a]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"a");
}

#[test]
fn seek_budget_and_guest_write_faults_preserve_cursor_and_eof() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    prepare(&mut p, SEEK, &[pointer, 5, 0]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    assert_eq!(success(&mut p), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"\x1a");
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 16, pointer]), 5);
    assert_eq!(word(&p, pointer + 12), 0x15);

    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, SEEK, &[pointer, 0, 0]);
    failure(&mut p, false);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(word(&p, pointer + 12), 0x15);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 0);

    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, SEEK, &[pointer, 0, 3]);
    failure(&mut p, false);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(word(&p, pointer + 12), 0x15);
    assert_eq!(call(&mut p, SEEK, &[pointer, 0, 0]), 0);

    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, SEEK, &[pointer, 1, 0]), 0);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"b");
}

#[test]
fn tell_reports_owned_cursor_without_changing_stream_or_error_state() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    p.memory
        .write(u64::from(ERRNO), &123_u32.to_le_bytes())
        .unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();

    prepare(&mut p, TELL, &[pointer]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    assert_eq!(success(&mut p), 0);
    assert_eq!(call(&mut p, TELL, &[pointer]), 0);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 5, pointer]), 5);
    assert_eq!(call(&mut p, TELL, &[pointer]), 5);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 16, pointer]), 6);
    assert_eq!(word(&p, pointer + 12), 0x15);
    assert_eq!(call(&mut p, TELL, &[pointer]), 11);
    assert_eq!(word(&p, pointer + 12), 0x15);
    assert_eq!(word(&p, ERRNO), 123);
    assert_eq!(p.last_error().unwrap(), 77);

    assert_eq!(call(&mut p, SEEK, &[pointer, 4, 2]), 0);
    assert_eq!(call(&mut p, TELL, &[pointer]), 15);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, TELL, &[pointer]), 15);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ_WRITE)
        .unwrap();

    assert_eq!(
        call(&mut p, SEEK, &[pointer, i32::MAX.cast_unsigned(), 0]),
        0
    );
    assert_eq!(call(&mut p, TELL, &[pointer]), i32::MAX.cast_unsigned());
    assert_eq!(call(&mut p, SEEK, &[pointer, 1, 1]), 0);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, TELL, &[pointer]);
    failure(&mut p, false);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(success(&mut p), u32::MAX);
    assert_eq!(word(&p, ERRNO), 22);
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn snapshots_outlive_inputs_and_paths_follow_cwd_case_and_slashes() {
    let mut source = file_stream_cases::PAYLOAD.to_vec();
    let mut p = Process32::load_with_options(
        &file_stream_cases::executable(),
        128,
        ProcessOptions {
            current_directory: b"C:\\folder",
            files: &[FileMetadata {
                path: b"C:\\folder\\sample.bin",
                size: source.len() as u64,
            }],
            file_contents: &[FileContents {
                path: b"c:\\FOLDER\\SAMPLE.bin",
                bytes: &source,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    source.fill(0);
    drop(source);
    p.memory.write(u64::from(PATH), b"./SAMPLE.bin\0").unwrap();
    let a = open(&mut p);
    p.memory.write(u64::from(MODE), b"r\0").unwrap();
    prepare(&mut p, OPEN, &[PATH, MODE]);
    failure(&mut p, true);
    p.memory
        .write(0x7000_2000, &0x8000_u32.to_le_bytes())
        .unwrap();
    let b = open(&mut p);
    for pointer in [a, b] {
        assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 11, pointer]), 11);
        assert_eq!(bytes(&p, OUTPUT, 11), file_stream_cases::PAYLOAD);
    }
}

#[test]
fn metadata_only_is_unsupported_but_supplied_empty_contents_open_and_reach_eof() {
    for supplied in [false, true] {
        let content = [FileContents {
            path: b"C:\\sample.bin",
            bytes: b"",
        }];
        let mut p = Process32::load_with_options(
            &file_stream_cases::executable(),
            128,
            ProcessOptions {
                files: &[FileMetadata {
                    path: b"C:\\sample.bin",
                    size: 0,
                }],
                file_contents: if supplied { &content } else { &[] },
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        prepare(&mut p, OPEN, &[PATH, MODE]);
        if supplied {
            let pointer = success(&mut p);
            assert_ne!(pointer, 0);
            assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 0);
            assert_eq!(word(&p, pointer + 12), 0x15);
        } else {
            failure(&mut p, true);
        }
    }
}

#[test]
fn read_faults_and_zero_budget_preserve_bytes_cursor_and_eof_for_retry() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    p.memory.write(u64::from(OUTPUT), &[0x55; 16]).unwrap();
    prepare(&mut p, READ, &[OUTPUT, 1, 16, pointer]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    failure(&mut p, false);
    for permission in [Permissions::READ, Permissions::NONE] {
        prepare(&mut p, READ, &[OUTPUT, 1, 16, pointer]);
        p.memory
            .protect(u64::from(pointer), 4096, permission)
            .unwrap();
        failure(&mut p, false);
        assert_eq!(bytes(&p, OUTPUT, 16), [0x55; 16]);
        p.memory
            .protect(u64::from(pointer), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(word(&p, pointer + 12), 5);
    }
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    for buffer in [0x3000_0000, 0x0040_2ffc, u32::MAX] {
        prepare(&mut p, READ, &[buffer, 1, 16, pointer]);
        failure(&mut p, false);
        assert_eq!(bytes(&p, OUTPUT, 16), [0x55; 16]);
        assert_eq!(word(&p, pointer + 12), 5);
    }
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 16, pointer]), 11);
    assert_eq!(bytes(&p, OUTPUT, 11), file_stream_cases::PAYLOAD);
}

#[test]
fn unsupported_requests_corrupt_records_and_wrong_heap_owner_do_not_consume_data() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    for args in [[0, 0, 0], [pointer + 4, 0, 0]] {
        prepare(&mut p, SEEK, &args);
        failure(&mut p, true);
    }
    for foreign in [0, pointer + 4] {
        prepare(&mut p, TELL, &[foreign]);
        failure(&mut p, true);
    }
    for args in [
        [OUTPUT, u32::MAX, 2, pointer],
        [OUTPUT, 1, 64 * 1024 * 1024 + 1, pointer],
        [0, 1, 1, pointer],
        [OUTPUT, 1, 1, 0],
        [pointer + 31, 1, 1, pointer],
    ] {
        prepare(&mut p, READ, &args);
        failure(&mut p, true);
    }
    p.memory.write(u64::from(pointer), &[1]).unwrap();
    prepare(&mut p, TELL, &[pointer]);
    failure(&mut p, true);
    prepare(&mut p, READ, &[OUTPUT, 1, 1, pointer]);
    failure(&mut p, true);
    prepare(&mut p, SEEK, &[pointer, 0, 0]);
    failure(&mut p, true);
    prepare(&mut p, CLOSE, &[pointer]);
    failure(&mut p, true);
    p.memory.write(u64::from(pointer), &[0]).unwrap();
    prepare(&mut p, 0x7000_0120, &[pointer]);
    failure(&mut p, true);
    prepare(&mut p, CLOSE, &[pointer]);
    p.cpu.set_register(Register32::Esp, pointer + 64);
    p.memory
        .write(u64::from(pointer + 64), &[0, 0x10, 0x40, 0])
        .unwrap();
    p.memory
        .write(u64::from(pointer + 68), &pointer.to_le_bytes())
        .unwrap();
    failure(&mut p, true);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 1);
    assert_eq!(bytes(&p, OUTPUT, 1), b"a");
    assert_eq!(call(&mut p, CLOSE, &[pointer]), 0);
    prepare(&mut p, TELL, &[pointer]);
    failure(&mut p, true);
    prepare(&mut p, SEEK, &[pointer, 0, 0]);
    failure(&mut p, true);
}

#[test]
fn open_failures_check_errno_before_effects_and_success_does_not_require_writable_errno() {
    let mut p = file_stream_cases::process(128);
    for path in [b"absent\0".as_slice(), b"C:\\\0"] {
        p.memory.write(u64::from(PATH), path).unwrap();
        prepare(&mut p, OPEN, &[PATH, MODE]);
        p.memory
            .protect(0x7000_2000, 4096, Permissions::READ)
            .unwrap();
        failure(&mut p, false);
        p.memory
            .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(success(&mut p), 0);
        assert_eq!(word(&p, ERRNO), if path[0] == b'C' { 13 } else { 2 });
    }
    p.memory.write(u64::from(PATH), b"sample.bin\0").unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    let pointer = open(&mut p);
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 1);
    assert_eq!(call(&mut p, CLOSE, &[pointer]), 0);
    for mode in [b"r\0".as_slice(), b"rt\0", b"wb\0", b"r+\0", b"rb+\0"] {
        p.memory.write(u64::from(MODE), mode).unwrap();
        prepare(&mut p, OPEN, &[PATH, MODE]);
        failure(&mut p, true);
    }
}

#[test]
fn stream_quota_and_heap_exhaustion_do_not_publish_owners_and_closed_slots_are_reused() {
    let mut p = file_stream_cases::process(1024);
    let pages = p.memory.mapped_pages();
    let pointers: Vec<_> = (0..512).map(|_| open(&mut p)).collect();
    assert_eq!(p.memory.mapped_pages(), pages + 512);
    assert_eq!(call(&mut p, OPEN, &[PATH, MODE]), 0);
    assert_eq!(word(&p, ERRNO), 24);
    assert_eq!(call(&mut p, CLOSE, &[pointers[50]]), 0);
    let replacement = open(&mut p);
    assert_eq!(replacement, pointers[50]);
    assert_eq!(word(&p, replacement + 16), 53);
    let mut limited = file_stream_cases::process(u32::try_from(pages).unwrap());
    assert_eq!(call(&mut limited, OPEN, &[PATH, MODE]), 0);
    assert_eq!(word(&limited, ERRNO), 12);
    assert_eq!(limited.memory.mapped_pages(), pages);
    assert_eq!(call(&mut limited, 0x7000_0188, &[PATH]), 0);
}

#[test]
fn explicit_output_aliases_update_the_real_return_slot_and_guest_error_cells() {
    for output in [STACK, ERRNO, 0x7ffd_e034] {
        let mut p = file_stream_cases::process(128);
        let pointer = open(&mut p);
        assert_eq!(call(&mut p, READ, &[output, 1, 4, pointer]), 4);
        assert_eq!(word(&p, output), 0x0d00_6261);
        if output == STACK {
            assert_eq!(p.cpu.eip, 0x0d00_6261);
        }
    }
}

#[test]
fn unchanged_file_flags_need_no_write_permission() {
    let mut p = file_stream_cases::process(128);
    let pointer = open(&mut p);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 11, pointer]), 11);
    assert_eq!(word(&p, pointer + 12), 5);
    prepare(&mut p, READ, &[OUTPUT, 1, 1, pointer]);
    failure(&mut p, false);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(success(&mut p), 0);
    assert_eq!(word(&p, pointer + 12), 0x15);
    p.memory
        .protect(u64::from(pointer), 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, READ, &[OUTPUT, 1, 1, pointer]), 0);
    assert_eq!(call(&mut p, CLOSE, &[pointer]), 0);
}
