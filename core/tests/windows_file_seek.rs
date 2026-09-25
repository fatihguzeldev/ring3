#[path = "support/file_seek_cases.rs"]
mod file_seek_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_seek_has_independent_position_and_preserves_size() {
    file_seek_cases::imported_seek_has_independent_position_and_preserves_size();
}

use file_seek_cases::{
    CLOSE, ERRNO, ERROR, FIRST, PATH, SEEK, call, open, prepare, process, put, seek, word,
};
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

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
fn handles_and_crt_streams_have_independent_positions_and_shared_pins() {
    let mut p = process(0, b"abcdef");
    assert_eq!(open(&mut p, 0), FIRST);
    let other = open(&mut p, 0x80);
    p.memory.write(0x0040_2300, b"rb\0").unwrap();
    let stream = call(&mut p, 0x7000_0194, &[PATH, 0x0040_2300]);
    assert_eq!(seek(&mut p, FIRST, 2, 0), 2);
    assert_eq!(seek(&mut p, FIRST, -1, 1), 1);
    assert_eq!(seek(&mut p, FIRST, -2, 2), 4);
    assert_eq!(seek(&mut p, other, 0, 1), 0);
    assert_eq!(call(&mut p, 0x7000_01a4, &[stream]), 0);
    assert_eq!(call(&mut p, 0x7000_01a0, &[stream, 3, 0]), 0);
    assert_eq!(seek(&mut p, other, 1, 2), 7);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 4);
    assert_eq!(call(&mut p, 0x7000_01a4, &[stream]), 3);
    assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST, 0]), 6);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut p, CLOSE, &[other]), 1);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), u32::MAX);
    assert_eq!(call(&mut p, 0x7000_019c, &[stream]), 0);
    assert_eq!(call(&mut p, 0x7000_0188, &[PATH]), 0);
}

#[test]
fn signed_distances_and_sentinel_success_do_not_resize_the_file() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    let pages = p.memory.mapped_pages();
    for distance in [-1, i32::MIN] {
        assert_eq!(seek(&mut p, FIRST, distance, 0), u32::MAX);
        assert_eq!(word(&p, ERROR), 131);
        assert_eq!(seek(&mut p, FIRST, 0, 1), 0);
    }
    assert_eq!(seek(&mut p, FIRST, i32::MAX, 0), 0x7fff_ffff);
    assert_eq!(seek(&mut p, FIRST, i32::MAX, 1), 0xffff_fffe);
    put(&mut p, ERROR, 77);
    assert_eq!(seek(&mut p, FIRST, 1, 1), u32::MAX);
    assert_eq!(word(&p, ERROR), 0);
    for distance in [1, i32::MAX] {
        assert_eq!(seek(&mut p, FIRST, distance, 1), u32::MAX);
        assert_eq!(word(&p, ERROR), 87);
        assert_eq!(seek(&mut p, FIRST, 0, 1), u32::MAX);
        assert_eq!(word(&p, ERROR), 0);
    }
    assert_eq!(seek(&mut p, FIRST, i32::MIN, 1), 0x7fff_ffff);
    assert_eq!(seek(&mut p, FIRST, -i32::MAX, 1), 0);
    assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST, 0]), 4);
    assert_eq!(word(&p, ERRNO), 88);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn unbuffered_alignment_checks_the_target_against_reported_logical_sectors() {
    for flags in [0, 0x80, 0x2000_0000, 0x2000_0080] {
        let mut p = process(flags, &vec![0x91; 1500]);
        assert_eq!(open(&mut p, flags), FIRST);
        p.memory.write(0x0040_2400, b"C:\\\0").unwrap();
        assert_eq!(
            call(
                &mut p,
                0x7000_05bc,
                &[
                    0x0040_2400,
                    0x0040_2500,
                    0x0040_2504,
                    0x0040_2508,
                    0x0040_250c
                ]
            ),
            1
        );
        assert_eq!(word(&p, 0x0040_2504), 512);
        assert_eq!(seek(&mut p, FIRST, 512, 0), 512);
        if flags & 0x2000_0000 != 0 {
            for (distance, origin) in [(-1, 1), (1, 0), (0, 2)] {
                assert_eq!(seek(&mut p, FIRST, distance, origin), u32::MAX);
                assert_eq!(word(&p, ERROR), 87);
                assert_eq!(seek(&mut p, FIRST, 0, 1), 512);
            }
            assert_eq!(seek(&mut p, FIRST, -476, 2), 1024);
            assert_eq!(seek(&mut p, FIRST, 36, 2), 1536);
            assert_eq!(seek(&mut p, FIRST, -2048, 1), u32::MAX);
            assert_eq!(word(&p, ERROR), 131);
            assert_eq!(seek(&mut p, FIRST, 0, 1), 1536);
        } else {
            assert_eq!(seek(&mut p, FIRST, -1, 1), 511);
            assert_eq!(seek(&mut p, FIRST, 0, 2), 1500);
        }
        assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST, 0]), 1500);
    }
}

#[test]
fn unsupported_profiles_and_invalid_actors_or_handles_do_not_move_positions() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    assert_eq!(seek(&mut p, FIRST, 3, 0), 3);
    for args in [
        [FIRST, 0, 0x5000_0000, 0],
        [FIRST, 0, u32::MAX, 0],
        [FIRST, 0, 0, 3],
        [FIRST, 0, 0, u32::MAX],
    ] {
        let before = prepare(&mut p, SEEK, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: SEEK });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, ERROR), 77);
        assert_eq!(seek(&mut p, FIRST, 0, 1), 3);
    }
    let fs = p.cpu.fs_base();
    p.cpu.set_fs_base(0x6000_0000);
    let before = prepare(&mut p, SEEK, &[FIRST, 2, 0, 0]);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: SEEK });
    assert_eq!(run.api_calls, 0);
    assert_eq!(p.cpu, before);
    p.cpu.set_fs_base(fs);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 2);
    let mutex = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    for invalid in [0, u32::MAX, u32::MAX - 1, FIRST + 1, FIRST + 4, mutex] {
        assert_eq!(seek(&mut p, invalid, 1, 0), u32::MAX);
        assert_eq!(word(&p, ERROR), 6);
        assert_eq!(seek(&mut p, FIRST, 0, 1), 2);
    }
    assert_eq!(call(&mut p, CLOSE, &[mutex]), 1);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(seek(&mut p, FIRST, 0, 1), u32::MAX);
    assert_eq!(word(&p, ERROR), 6);
}

#[test]
fn error_write_faults_preserve_positions_and_sentinel_success_is_atomic() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(seek(&mut p, FIRST, 3, 0), 3);
    prepare(&mut p, SEEK, &[FIRST, (-4_i32).cast_unsigned(), 0, 1]);
    fault(&mut p);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 3);
    assert_eq!(seek(&mut p, FIRST, i32::MAX, 0), 0x7fff_ffff);
    assert_eq!(seek(&mut p, FIRST, i32::MAX, 1), 0xffff_fffe);
    prepare(&mut p, SEEK, &[FIRST, 1, 0, 1]);
    fault(&mut p);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 0xffff_fffe);
    assert_eq!(word(&p, ERROR), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, SEEK, &[FIRST, 1, 0, 1]);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, ERROR), 0);
    assert_eq!(seek(&mut p, FIRST, 0, 1), u32::MAX);
}

#[test]
fn incomplete_and_top_address_frames_do_not_move_the_cursor() {
    let mut p = process(0, b"data");
    assert_eq!(open(&mut p, 0), FIRST);
    assert_eq!(seek(&mut p, FIRST, 2, 0), 2);
    p.memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let frame = [0x0040_1000, FIRST, 3, 0, 0];
    for (i, value) in frame[..4].iter().enumerate() {
        put(&mut p, 0x6000_0ff0 + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu.eip = SEEK;
    p.cpu.set_register(Register32::Esp, 0x6000_0ff0);
    fault(&mut p);
    let retry = p.cpu;
    assert_eq!(seek(&mut p, FIRST, 0, 1), 2);
    p.memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x6000_1000, 0);
    p.cpu = retry;
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 3);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (i, value) in [0x0040_1000, FIRST, 1, 0, 1].into_iter().enumerate() {
        put(&mut p, 0xffff_ffec + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.eip = SEEK;
    p.cpu.set_register(Register32::Esp, 0xffff_ffec);
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
    );
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(seek(&mut p, FIRST, 0, 1), 3);
}

#[test]
fn empty_files_and_protected_original_paths_keep_owned_position() {
    for flags in [0, 0x2000_0080] {
        let mut p = process(flags, b"");
        assert_eq!(open(&mut p, flags), FIRST);
        assert_eq!(seek(&mut p, FIRST, 0, 2), 0);
        p.memory.write(u64::from(PATH), b"different.bin\0").unwrap();
        p.memory
            .protect(0x0040_2000, 4096, Permissions::NONE)
            .unwrap();
        let pages = p.memory.mapped_pages();
        assert_eq!(seek(&mut p, FIRST, 512, 0), 512);
        assert_eq!(seek(&mut p, FIRST, -512, 1), 0);
        assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST, 0]), 0);
        assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
        assert_eq!(word(&p, ERROR), 77);
        assert_eq!(p.memory.mapped_pages(), pages);
    }
}
