use super::file_read_cases;

#[test]
fn imported_reads_copy_real_bytes_and_stop_at_eof() {
    file_read_cases::imported_reads_copy_real_bytes_and_stop_at_eof();
}

use file_read_cases::{
    BUFFER, CLOSE, COUNT, ERRNO, ERROR, FIRST, PATH, READ, SEEK, STACK, bytes, call, open, prepare,
    process, put, read, word,
};
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn position(p: &mut Process32, handle: u32) -> u32 {
    call(p, SEEK, &[handle, 0, 0, 1])
}

fn fault(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn short_reads_eof_seek_and_crt_cursors_are_independent() {
    let mut p = process(0, b"abcdef");
    assert_eq!(open(&mut p, 0), FIRST);
    let other = open(&mut p, 0);
    p.memory.write(0x0040_2400, b"rb\0").unwrap();
    let stream = call(&mut p, 0x7000_0194, &[PATH, 0x0040_2400]);
    for (expected, actual, cursor) in [
        (b"abcd".as_slice(), 4, 4),
        (b"ef".as_slice(), 2, 6),
        (b"".as_slice(), 0, 6),
    ] {
        p.memory.write(u64::from(BUFFER), &[0xa5; 8]).unwrap();
        assert_eq!(read(&mut p, FIRST, BUFFER, 4), 1);
        assert_eq!(word(&p, COUNT), actual);
        assert_eq!(bytes(&p, BUFFER, expected.len()), expected);
        assert_eq!(
            bytes(&p, BUFFER + actual, 8 - actual as usize),
            vec![0xa5; 8 - actual as usize]
        );
        assert_eq!(position(&mut p, FIRST), cursor);
    }
    assert_eq!(position(&mut p, other), 0);
    assert_eq!(call(&mut p, 0x7000_01a4, &[stream]), 0);
    assert_eq!(call(&mut p, 0x7000_0198, &[BUFFER, 1, 2, stream]), 2);
    assert_eq!(bytes(&p, BUFFER, 2), b"ab");
    assert_eq!(position(&mut p, FIRST), 6);
    assert_eq!(read(&mut p, other, BUFFER, 3), 1);
    assert_eq!(bytes(&p, BUFFER, 3), b"abc");
    assert_eq!(call(&mut p, SEEK, &[FIRST, 1, 0, 0]), 1);
    assert_eq!(read(&mut p, FIRST, BUFFER, 3), 1);
    assert_eq!(bytes(&p, BUFFER, 3), b"bcd");
    assert_eq!(position(&mut p, other), 3);
    assert_eq!(call(&mut p, 0x7000_01a4, &[stream]), 2);
    assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST, 0]), 6);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut p, CLOSE, &[other]), 1);
    assert_eq!(call(&mut p, 0x7000_019c, &[stream]), 0);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), 0);
}

#[test]
fn contents_are_owned_and_zero_or_empty_reads_do_not_access_the_data_pointer() {
    let mut source = b"owned".to_vec();
    let mut p = process(0, &source);
    source.fill(b'x');
    assert_eq!(open(&mut p, 0), FIRST);
    p.memory.write(u64::from(PATH), b"changed\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, READ, &[FIRST, BUFFER, 8, STACK + 64, 0]), 1);
    assert_eq!(bytes(&p, BUFFER, 5), b"owned");
    assert_eq!(word(&p, STACK + 64), 5);
    for pointer in [0, u32::MAX, 0x5000_0001] {
        assert_eq!(call(&mut p, READ, &[FIRST, pointer, 0, STACK + 64, 0]), 1);
        assert_eq!(word(&p, STACK + 64), 0);
        assert_eq!(position(&mut p, FIRST), 5);
    }
    assert_eq!(word(&p, ERROR), 77);
    assert_eq!(word(&p, ERRNO), 88);
    assert_eq!(p.memory.mapped_pages(), pages);
    for flags in [0, 0x2000_0080] {
        let mut p = process(flags, b"");
        assert_eq!(open(&mut p, flags), FIRST);
        assert_eq!(read(&mut p, FIRST, BUFFER, 512), 1);
        assert_eq!(word(&p, COUNT), 0);
        assert_eq!(bytes(&p, BUFFER, 512), vec![0xa5; 512]);
        assert_eq!(position(&mut p, FIRST), 0);
    }
}

#[test]
fn full_requested_buffer_is_preflighted_for_short_reads_and_at_eof() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    let output = BUFFER + 8190;
    prepare(&mut p, READ, &[FIRST, output, 8, COUNT, 0]);
    fault(&mut p);
    assert_eq!(bytes(&p, output, 2), [0xa5; 2]);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    assert_eq!(position(&mut p, FIRST), 0);
    p.memory
        .map_zeroed(u64::from(BUFFER + 8192), 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, READ, &[FIRST, output, 8, COUNT, 0]);
    fault(&mut p);
    assert_eq!(bytes(&p, output, 2), [0xa5; 2]);
    p.memory
        .protect(
            u64::from(BUFFER + 8192),
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    p.memory
        .protect(u64::from(BUFFER + 8192), 4096, Permissions::READ)
        .unwrap();
    assert_eq!(bytes(&p, output, 4), b"data");
    assert_eq!(word(&p, COUNT), 4);
    assert_eq!(position(&mut p, FIRST), 4);
    prepare(&mut p, READ, &[FIRST, output, 8, COUNT, 0]);
    fault(&mut p);
    assert_eq!(word(&p, COUNT), 4);
    p.memory
        .protect(u64::from(BUFFER + 8192), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, COUNT), 0);
    assert_eq!(position(&mut p, FIRST), 4);
    assert_eq!(word(&p, ERROR), 77);
}

#[test]
fn count_output_and_error_page_are_preflighted_before_any_publication() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    p.memory
        .map_zeroed(0x6100_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6100_0ffe, &[0x11, 0x22]).unwrap();
    prepare(&mut p, READ, &[FIRST, BUFFER, 4, 0x6100_0ffe, 0]);
    fault(&mut p);
    assert_eq!(bytes(&p, BUFFER, 4), [0xa5; 4]);
    assert_eq!(bytes(&p, 0x6100_0ffe, 2), [0x11, 0x22]);
    p.memory
        .map_zeroed(
            0x6100_1000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    p.memory
        .protect(0x6100_1000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(word(&p, 0x6100_0ffe), 4);
    assert_eq!(bytes(&p, BUFFER, 4), b"data");
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, READ, &[FIRST + 4, u32::MAX, 4, COUNT, 0]);
    fault(&mut p);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    assert_eq!(word(&p, ERROR), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, COUNT), 0);
    assert_eq!(word(&p, ERROR), 6);
    assert_eq!(position(&mut p, FIRST), 4);
}

#[test]
fn unbuffered_reads_enforce_virtual_alignment_and_recover_after_short_eof() {
    for flags in [0, 0x80, 0x2000_0000, 0x2000_0080] {
        let mut p = process(flags, &vec![0x91; 513]);
        assert_eq!(open(&mut p, flags), FIRST);
        if flags & 0x2000_0000 != 0 {
            for (output, requested) in [(BUFFER + 1, 512), (BUFFER, 1)] {
                assert_eq!(read(&mut p, FIRST, output, requested), 0);
                assert_eq!(word(&p, COUNT), 0);
                assert_eq!(word(&p, ERROR), 87);
                assert_eq!(position(&mut p, FIRST), 0);
            }
        } else {
            assert_eq!(read(&mut p, FIRST, BUFFER + 1, 1), 1);
            assert_eq!(word(&p, COUNT), 1);
            assert_eq!(call(&mut p, SEEK, &[FIRST, 0, 0, 0]), 0);
        }
        assert_eq!(read(&mut p, FIRST, BUFFER, 512), 1);
        assert_eq!(read(&mut p, FIRST, BUFFER + 512, 512), 1);
        assert_eq!(word(&p, COUNT), 1);
        assert_eq!(read(&mut p, FIRST, u32::MAX, 0), 1);
        assert_eq!(word(&p, COUNT), 0);
        if flags & 0x2000_0000 != 0 {
            assert_eq!(read(&mut p, FIRST, BUFFER, 512), 0);
            assert_eq!(word(&p, ERROR), 87);
        } else {
            assert_eq!(read(&mut p, FIRST, BUFFER, 512), 1);
            assert_eq!(word(&p, COUNT), 0);
        }
        assert_eq!(call(&mut p, SEEK, &[FIRST, 511, 0, 1]), 1024);
        assert_eq!(read(&mut p, FIRST, BUFFER, 512), 1);
        assert_eq!(word(&p, COUNT), 0);
        assert_eq!(position(&mut p, FIRST), 1024);
    }
}

#[test]
fn unsupported_profiles_and_actors_preserve_count_and_cursor() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    for args in [
        [FIRST, BUFFER, 4, 0, 0],
        [FIRST, BUFFER, 4, COUNT, 0x5000_0000],
        [FIRST, BUFFER, 64 * 1024 * 1024 + 1, COUNT, 0],
        [FIRST, BUFFER, u32::MAX, COUNT, 0],
    ] {
        let before = prepare(&mut p, READ, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: READ });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, COUNT), 0xfeed_abba);
        assert_eq!(position(&mut p, FIRST), 0);
    }
    prepare(&mut p, READ, &[FIRST, BUFFER, 64 * 1024 * 1024, COUNT, 0]);
    fault(&mut p);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    assert_eq!(position(&mut p, FIRST), 0);
    let fs = p.cpu.fs_base();
    p.cpu.set_fs_base(0x6500_0000);
    let before = prepare(&mut p, READ, &[FIRST, BUFFER, 4, COUNT, 0]);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: READ });
    assert_eq!(p.cpu, before);
    assert_eq!(run.api_calls, 0);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    p.cpu.set_fs_base(fs);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(bytes(&p, BUFFER, 4), b"data");
    assert_eq!(position(&mut p, FIRST), 4);
}

#[test]
fn failed_reads_zero_the_count_and_error_aliases_publish_last_error_last() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    let mutex = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    for invalid in [0, u32::MAX, u32::MAX - 1, FIRST + 1, FIRST + 4, mutex] {
        put(&mut p, COUNT, 0xfeed_abba);
        assert_eq!(read(&mut p, invalid, u32::MAX, 4), 0);
        assert_eq!(word(&p, COUNT), 0);
        assert_eq!(word(&p, ERROR), 6);
        assert_eq!(bytes(&p, BUFFER, 8), [0xa5; 8]);
        assert_eq!(position(&mut p, FIRST), 0);
    }
    assert_eq!(call(&mut p, READ, &[FIRST + 4, BUFFER, 8, BUFFER, 0]), 0);
    assert_eq!(word(&p, BUFFER), 0);
    assert_eq!(bytes(&p, BUFFER + 4, 4), [0xa5; 4]);
    assert_eq!(call(&mut p, READ, &[FIRST + 4, BUFFER, 8, ERROR, 0]), 0);
    assert_eq!(word(&p, ERROR), 6);
    assert_eq!(call(&mut p, CLOSE, &[mutex]), 1);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(read(&mut p, FIRST, BUFFER, 4), 0);
    assert_eq!(word(&p, COUNT), 0);
    assert_eq!(open(&mut p, 0), FIRST + 4);
    assert_eq!(position(&mut p, FIRST + 4), 0);
}

#[test]
fn data_then_count_aliases_preserve_owned_bytes_and_live_return_semantics() {
    for (output, count) in [
        (BUFFER, BUFFER + 2),
        (BUFFER, STACK),
        (BUFFER, STACK + 4),
        (STACK, COUNT),
        (BUFFER, ERROR),
        (BUFFER, ERRNO),
    ] {
        let mut p = process(0, b"abcdefgh");
        assert_eq!(open(&mut p, 0), FIRST);
        let mut expected = prepare(&mut p, READ, &[FIRST, output, 8, count, 0]);
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (0, 1));
        expected.eip = if count == STACK {
            8
        } else if output == STACK {
            u32::from_le_bytes(*b"abcd")
        } else {
            0x0040_1000
        };
        expected.set_register(Register32::Esp, STACK + 24);
        expected.set_register(Register32::Eax, 1);
        assert_eq!(p.cpu, expected);
        let mut expected_bytes = std::collections::BTreeMap::new();
        for (offset, byte) in b"abcdefgh".iter().enumerate() {
            expected_bytes.insert(output + u32::try_from(offset).unwrap(), *byte);
        }
        for (offset, byte) in 8_u32.to_le_bytes().into_iter().enumerate() {
            expected_bytes.insert(count + u32::try_from(offset).unwrap(), byte);
        }
        for (address, byte) in expected_bytes {
            assert_eq!(bytes(&p, address, 1), [byte]);
        }
        assert_eq!(position(&mut p, FIRST), 8);
        assert_eq!(call(&mut p, SEEK, &[FIRST, 0, 0, 0]), 0);
        assert_eq!(read(&mut p, FIRST, BUFFER + 512, 8), 1);
        assert_eq!(bytes(&p, BUFFER + 512, 8), b"abcdefgh");
    }
}

#[test]
fn address_overflow_and_incomplete_or_top_frames_never_publish_partial_reads() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, &[0x11, 0x22]).unwrap();
    for args in [
        [FIRST, 0xffff_fffe, 4, COUNT, 0],
        [FIRST, BUFFER, 4, 0xffff_fffe, 0],
    ] {
        let before = prepare(&mut p, READ, &args);
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, COUNT), 0xfeed_abba);
        assert_eq!(bytes(&p, BUFFER, 4), [0xa5; 4]);
        assert_eq!(bytes(&p, 0xffff_fffe, 2), [0x11, 0x22]);
        assert_eq!(position(&mut p, FIRST), 0);
    }
    p.memory
        .map_zeroed(0x6200_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let frame = [0x0040_1000, FIRST, BUFFER, 4, COUNT, 0];
    for (i, value) in frame[..5].iter().enumerate() {
        put(&mut p, 0x6200_0fec + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu.eip = READ;
    p.cpu.set_register(Register32::Esp, 0x6200_0fec);
    fault(&mut p);
    let retry = p.cpu;
    assert_eq!(position(&mut p, FIRST), 0);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    p.memory
        .map_zeroed(0x6200_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x6200_1000, 0);
    p.cpu = retry;
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(bytes(&p, BUFFER, 4), b"data");
    assert_eq!(call(&mut p, SEEK, &[FIRST, 0, 0, 0]), 0);
    p.memory.write(u64::from(BUFFER), &[0xa5; 4]).unwrap();
    put(&mut p, COUNT, 0xfeed_abba);
    for (i, value) in frame.into_iter().enumerate() {
        put(&mut p, 0xffff_ffe8 + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.eip = READ;
    p.cpu.set_register(Register32::Esp, 0xffff_ffe8);
    fault(&mut p);
    assert_eq!(bytes(&p, BUFFER, 4), [0xa5; 4]);
    assert_eq!(word(&p, COUNT), 0xfeed_abba);
    assert_eq!(position(&mut p, FIRST), 0);
}
