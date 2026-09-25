use super::file_handle_cases;

#[test]
fn imported_open_close_preserves_state_and_releases_identity() {
    file_handle_cases::imported_open_close_preserves_state_and_releases_identity();
}

use file_handle_cases::{
    CLOSE, ERRNO, ERROR, FIRST, OPEN, PATH, STACK, args, call, prepare, process, word,
};
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const FOPEN: u32 = 0x7000_0194;
const FCLOSE: u32 = 0x7000_019c;
const REMOVE: u32 = 0x7000_0188;

fn path(p: &mut Process32, name: &[u8]) {
    p.memory.write(u64::from(PATH), name).unwrap();
}

fn rejected(p: &mut Process32, api: u32, arguments: &[u32], reason: &ProcessStop) {
    let before = prepare(p, api, arguments);
    let run = p.run(1);
    assert_eq!(&run.reason, reason);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn normalized_paths_are_owned_and_close_needs_no_path_or_error_page_access() {
    for name in [
        &b"SAMPLE.bin\0"[..],
        b".\\sample.bin\0",
        b"..\\root\\sample.bin\0",
        b"C:\\\\root\\sample.bin\0",
        b"C:/root/sample.bin\0",
        b"\\root\\sample.bin\0",
        b"empty.bin\0",
    ] {
        let mut p = process(0x80);
        path(&mut p, name);
        p.memory
            .protect(0x7ffd_e000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(call(&mut p, OPEN, &args(0x80)), FIRST);
        path(&mut p, b"different.bin\0");
        p.memory
            .protect(0x0040_2000, 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
        p.memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(word(&p, ERROR), 77);
        assert_eq!(word(&p, ERRNO), 88);
    }
}

#[test]
fn crt_streams_and_win32_handles_keep_shared_content_pinned_until_final_close() {
    for crt_first in [false, true] {
        let mut p = process(0x80);
        p.memory.write(0x0040_2300, b"rb\0").unwrap();
        let (stream, handle) = if crt_first {
            let stream = call(&mut p, FOPEN, &[PATH, 0x0040_2300]);
            (stream, call(&mut p, OPEN, &args(0x80)))
        } else {
            let handle = call(&mut p, OPEN, &args(0x80));
            (call(&mut p, FOPEN, &[PATH, 0x0040_2300]), handle)
        };
        assert_ne!(stream, 0);
        assert_eq!(handle, FIRST);
        let second = call(&mut p, OPEN, &args(0x2000_0080));
        assert_eq!(call(&mut p, REMOVE, &[PATH]), u32::MAX);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
        assert_eq!(call(&mut p, REMOVE, &[PATH]), u32::MAX);
        assert_eq!(call(&mut p, FCLOSE, &[stream]), 0);
        assert_eq!(call(&mut p, REMOVE, &[PATH]), u32::MAX);
        assert_eq!(call(&mut p, CLOSE, &[second]), 1);
        assert_eq!(call(&mut p, REMOVE, &[PATH]), 0);
        assert_eq!(call(&mut p, OPEN, &args(0x80)), u32::MAX);
        assert_eq!(word(&p, ERROR), 2);
        path(&mut p, b"empty.bin\0");
        assert_eq!(call(&mut p, OPEN, &args(0)), FIRST + 8);
    }
}

#[test]
fn file_handles_do_not_alias_search_sync_or_crt_objects() {
    let mut p = process(0);
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    for (api, arguments, result) in [
        (0x7000_0214, vec![FIRST, 0], u32::MAX),
        (0x7000_0218, vec![FIRST], 0),
        (0x7000_0540, vec![FIRST], 0),
        (0x7000_023c, vec![FIRST], 0),
    ] {
        assert_eq!(call(&mut p, api, &arguments), result);
        assert_eq!(word(&p, ERROR), 6);
    }
    rejected(
        &mut p,
        FCLOSE,
        &[FIRST],
        &ProcessStop::UnsupportedApi { address: FCLOSE },
    );
    path(&mut p, b"*\0");
    let search = call(&mut p, 0x7000_0234, &[PATH, 0x0040_2400]);
    assert_eq!(search, 0x7300_0004);
    assert_eq!(call(&mut p, CLOSE, &[search]), 0);
    assert_eq!(call(&mut p, 0x7000_0238, &[search, 0x0040_2400]), 1);
    assert_eq!(call(&mut p, 0x7000_023c, &[search]), 1);
    let mutex = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    assert_eq!(mutex, 0x7200_0004);
    assert_eq!(call(&mut p, CLOSE, &[mutex]), 1);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    for invalid in [0, 1, FIRST, FIRST + 1, FIRST + 2, FIRST + 4] {
        assert_eq!(call(&mut p, CLOSE, &[invalid]), 0);
        assert_eq!(word(&p, ERROR), 6);
    }
}

#[test]
fn path_failures_preserve_win32_errors_without_allocating_a_handle() {
    let mut p = process(0);
    for (name, error) in [
        (&b"absent.bin\0"[..], 2),
        (b"missing\\absent.bin\0", 3),
        (b".\0", 5),
        (b".\\\0", 5),
        (b"sample.bin\\\0", 3),
        (b"bad?.bin\0", 123),
    ] {
        path(&mut p, name);
        assert_eq!(call(&mut p, OPEN, &args(0)), u32::MAX);
        assert_eq!(word(&p, ERROR), error);
    }
    for name in [
        &b"metadata.bin\0"[..],
        b"NUL\0",
        b"\\\\server\\file\0",
        b"C:sample.bin\0",
    ] {
        path(&mut p, name);
        rejected(
            &mut p,
            OPEN,
            &args(0),
            &ProcessStop::UnsupportedApi { address: OPEN },
        );
    }
    path(&mut p, b"sample.bin\0");
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    assert_eq!(word(&p, ERRNO), 88);
}

#[test]
fn unsupported_profiles_and_unknown_actor_do_not_publish_or_close_handles() {
    for (index, value) in [
        (1, 0),
        (1, 0x4000_0000),
        (2, 0),
        (2, 3),
        (3, 1),
        (4, 1),
        (4, 2),
        (4, 4),
        (5, 0x4000_0080),
        (5, 0x0200_0080),
        (5, 0x81),
    ] {
        let mut p = process(0);
        let mut values = args(0);
        values[index] = value;
        values[0] = 0;
        rejected(
            &mut p,
            OPEN,
            &values,
            &ProcessStop::UnsupportedApi { address: OPEN },
        );
        assert_eq!(word(&p, ERROR), 77);
        assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    }
    let mut p = process(0);
    p.cpu.set_fs_base(0x6000_0000);
    rejected(
        &mut p,
        OPEN,
        &args(0),
        &ProcessStop::UnsupportedApi { address: OPEN },
    );
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    p.cpu.set_fs_base(0x6000_0000);
    rejected(
        &mut p,
        CLOSE,
        &[FIRST],
        &ProcessStop::UnsupportedApi { address: CLOSE },
    );
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
}

#[test]
fn path_read_faults_and_finite_scan_refusal_leave_identity_available() {
    for source in [0, 0x5000_0000, 0xffff_fffe] {
        let mut p = process(0);
        if source == 0xffff_fffe {
            p.memory
                .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
                .unwrap();
            p.memory.write(u64::from(source), b"ab").unwrap();
        }
        let mut arguments = args(0);
        arguments[0] = source;
        let before = prepare(&mut p, OPEN, &arguments);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    }
    let mut p = process(0);
    p.memory
        .map_zeroed(0x6000_0000, 32768, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_0000, &vec![b'a'; 32768]).unwrap();
    let mut arguments = args(0);
    arguments[0] = 0x6000_0000;
    rejected(
        &mut p,
        OPEN,
        &arguments,
        &ProcessStop::UnsupportedApi { address: OPEN },
    );
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, OPEN, &args(0));
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), FIRST);
}

#[test]
fn complete_top_address_frames_refuse_before_open_or_close_mutates_ownership() {
    let mut p = process(0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for api in [OPEN, CLOSE] {
        let arguments = if api == OPEN {
            args(0).to_vec()
        } else {
            vec![FIRST]
        };
        prepare(&mut p, api, &arguments);
        let frame_size = (arguments.len() + 1) * 4;
        let mut frame = vec![0; frame_size];
        p.memory.read(u64::from(STACK), &mut frame).unwrap();
        let high = u32::MAX - u32::try_from(frame_size).unwrap() + 1;
        p.memory.write(u64::from(high), &frame).unwrap();
        p.cpu.set_register(Register32::Esp, high);
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        if api == OPEN {
            assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
        } else {
            assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
        }
    }
    assert_eq!(call(&mut p, REMOVE, &[PATH]), 0);
}

#[test]
fn split_frame_and_error_write_faults_are_atomic_and_retryable() {
    let mut p = process(0);
    prepare(&mut p, OPEN, &args(0));
    let mut frame = [0; 32];
    p.memory.read(u64::from(STACK), &mut frame).unwrap();
    p.memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_0ff0, &frame[..16]).unwrap();
    p.cpu.set_register(Register32::Esp, 0x6000_0ff0);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_1000, &frame[16..]).unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), FIRST);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    path(&mut p, b"absent.bin\0");
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, OPEN, &args(0));
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(word(&p, ERROR), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
    assert_eq!(word(&p, ERROR), 2);
    path(&mut p, b"sample.bin\0");
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST + 4);
    assert_eq!(call(&mut p, CLOSE, &[FIRST + 4]), 1);
}

#[test]
fn live_capacity_recovery_and_process_local_tables_preserve_pins() {
    let mut p = process(0);
    let mut other = process(0);
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST);
    assert_eq!(call(&mut other, CLOSE, &[FIRST]), 0);
    assert_eq!(call(&mut other, OPEN, &args(0x2000_0080)), FIRST);
    for index in 1..4096 {
        assert_eq!(call(&mut p, OPEN, &args(0)), FIRST + index * 4);
    }
    assert_eq!(call(&mut p, OPEN, &args(0)), u32::MAX);
    assert_eq!(word(&p, ERROR), 8);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut p, OPEN, &args(0)), FIRST + 4096 * 4);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 0);
    for index in 1..=4096 {
        assert_eq!(call(&mut p, CLOSE, &[FIRST + index * 4]), 1);
    }
    assert_eq!(call(&mut p, REMOVE, &[PATH]), 0);
    assert_eq!(call(&mut other, REMOVE, &[PATH]), u32::MAX);
    assert_eq!(call(&mut other, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut other, REMOVE, &[PATH]), 0);
    assert_eq!(p.memory.mapped_pages(), pages);
}
