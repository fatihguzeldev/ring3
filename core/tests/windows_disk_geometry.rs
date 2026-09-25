#[path = "support/disk_geometry_cases.rs"]
mod disk_geometry_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_geometry_is_deterministic_and_drive_local() {
    disk_geometry_cases::imported_geometry_is_deterministic_and_drive_local();
}

use disk_geometry_cases::{
    API, ERRNO, ERROR, OUT, ROOT, STACK, arguments, call, fixture, prepare, process, put, values,
    word,
};
use ring3_core::execution::{
    FileMetadata, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn unsupported(p: &mut Process32, args: &[u32]) {
    let before = prepare(p, API, args);
    let output = values(p);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(values(p), output);
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
fn capacity_is_fixed_and_only_successful_removal_reclaims_clusters() {
    let mut p = fixture();
    p.memory.write(0x0040_2400, b"C:\\root\\one\0").unwrap();
    let handle = call(
        &mut p,
        0x7000_05b4,
        &[0x0040_2400, 0x8000_0000, 1, 0, 3, 0, 0],
    );
    assert_eq!(call(&mut p, 0x7000_0188, &[0x0040_2400]), u32::MAX);
    assert_eq!(call(&mut p, API, &arguments()), 1);
    assert_eq!(values(&p), [8, 512, 0, 5]);
    assert_eq!(call(&mut p, 0x7000_021c, &[handle]), 1);
    for (name, free) in [
        (&b"C:\\root\\one\0"[..], 1),
        (b"C:\\root\\empty\0", 1),
        (b"C:\\root\\spill\0", 3),
        (b"C:\\root\\cluster\0", 4),
    ] {
        p.memory.write(0x0040_2400, name).unwrap();
        assert_eq!(call(&mut p, 0x7000_0188, &[0x0040_2400]), 0);
        assert_eq!(call(&mut p, API, &arguments()), 1);
        assert_eq!(values(&p), [8, 512, free, 5]);
        assert_eq!(call(&mut p, 0x7000_0188, &[0x0040_2400]), u32::MAX);
        assert_eq!(call(&mut p, API, &arguments()), 1);
        assert_eq!(values(&p), [8, 512, free, 5]);
    }
    p.memory.write(u64::from(ROOT), b"D:\\\0").unwrap();
    assert_eq!(call(&mut p, API, &arguments()), 1);
    assert_eq!(values(&p), [8, 512, 0, 4]);
    let mut independent = fixture();
    assert_eq!(call(&mut independent, API, &arguments()), 1);
    assert_eq!(values(&independent), [8, 512, 0, 5]);
}

#[test]
fn null_root_tracks_the_current_drive_and_success_needs_no_error_write() {
    let mut p = fixture();
    p.memory.write(0x0040_2400, b"d:\\root\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0230, &[0x0040_2400]), 1);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let mut args = arguments();
    args[0] = 0;
    assert_eq!(call(&mut p, API, &args), 1);
    assert_eq!(values(&p), [8, 512, 0, 4]);
    assert_eq!(word(&p, ERROR), 77);
    assert_eq!(word(&p, ERRNO), 88);
    assert_eq!(call(&mut p, API, &arguments()), 1);
    assert_eq!(values(&p), [8, 512, 0, 5]);
}

#[test]
fn counts_are_checked_without_clamping_or_cross_drive_contamination() {
    let largest = u64::from(u32::MAX - 1) * 4096;
    let mut p = process(
        b"C:\\",
        &[
            FileMetadata {
                path: b"C:\\large",
                size: largest,
            },
            FileMetadata {
                path: b"D:\\huge",
                size: u64::MAX,
            },
        ],
        &[],
    );
    assert_eq!(call(&mut p, API, &arguments()), 1);
    assert_eq!(values(&p), [8, 512, 0, u32::MAX]);
    p.memory.write(0x0040_2400, b"C:\\large\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0188, &[0x0040_2400]), 0);
    assert_eq!(call(&mut p, API, &arguments()), 1);
    assert_eq!(values(&p), [8, 512, u32::MAX - 1, u32::MAX]);
    for size in [largest + 1, u64::from(u32::MAX) * 4096, u64::MAX] {
        let mut p = process(
            b"C:\\",
            &[FileMetadata {
                path: b"C:\\large",
                size,
            }],
            &[],
        );
        unsupported(&mut p, &arguments());
        assert_eq!(word(&p, ERROR), 77);
    }
    let mut p = process(
        b"C:\\",
        &[
            FileMetadata {
                path: b"C:\\large",
                size: largest,
            },
            FileMetadata {
                path: b"C:\\extra",
                size: 1,
            },
        ],
        &[],
    );
    unsupported(&mut p, &arguments());
    for (size, total) in [(0, 1), (1, 2), (4095, 2), (4096, 2), (4097, 3)] {
        let mut p = process(
            b"C:\\",
            &[FileMetadata {
                path: b"C:\\size",
                size,
            }],
            &[],
        );
        assert_eq!(call(&mut p, API, &arguments()), 1);
        assert_eq!(values(&p), [8, 512, 0, total]);
    }
}

#[test]
fn unsupported_shapes_and_unknown_roots_do_not_publish_outputs() {
    let mut p = fixture();
    for root in [
        &b"\0"[..],
        b"C:\0",
        b"C:/\0",
        b"C:\\\\\0",
        b"C:\\root\0",
        b"\\\\host\\share\\\0",
        b"1:\\\0",
        b"relative\0",
    ] {
        p.memory.write(u64::from(ROOT), root).unwrap();
        unsupported(&mut p, &arguments());
    }
    for index in 1..5 {
        let mut args = arguments();
        args[0] = 0x5000_0000;
        args[index] = 0;
        unsupported(&mut p, &args);
    }
    p.memory.write(u64::from(ROOT), b"Z:\\\0").unwrap();
    let mut args = arguments();
    args[4] = u32::MAX;
    assert_eq!(call(&mut p, API, &args), 0);
    assert_eq!(word(&p, ERROR), 3);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
    put(&mut p, ERROR, 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, &args);
    fault(&mut p);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
    assert_eq!(word(&p, ERROR), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, ERROR), 3);
}

#[test]
fn every_output_is_checked_before_any_write_and_faults_are_retryable() {
    for index in 1..5 {
        let mut p = fixture();
        p.memory
            .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(0x6000_0ffe, &[0x55, 0x66]).unwrap();
        let mut args = arguments();
        args[index] = 0x6000_0ffe;
        prepare(&mut p, API, &args);
        fault(&mut p);
        assert_eq!(values(&p), [0xfeed_abba; 4]);
        let mut prefix = [0; 2];
        p.memory.read(0x6000_0ffe, &mut prefix).unwrap();
        assert_eq!(prefix, [0x55, 0x66]);
        p.memory
            .map_zeroed(0x6000_1000, 4096, Permissions::READ)
            .unwrap();
        fault(&mut p);
        assert_eq!(values(&p), [0xfeed_abba; 4]);
        p.memory
            .protect(
                0x6000_1000,
                4096,
                Permissions {
                    write: true,
                    ..Permissions::NONE
                },
            )
            .unwrap();
        assert_eq!(p.run(1).api_calls, 1);
        p.memory
            .protect(0x6000_1000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(word(&p, 0x6000_0ffe), [8, 512, 0, 5][index - 1]);
        for (position, &address) in args[1..].iter().enumerate() {
            assert_eq!(word(&p, address), [8, 512, 0, 5][position]);
        }
        assert_eq!(word(&p, ERROR), 77);
    }
}

#[test]
fn aliasing_uses_captured_input_and_parameter_order_then_rereads_return() {
    for outputs in [
        [OUT; 4],
        [ROOT; 4],
        [OUT, OUT + 1, OUT + 2, OUT + 3],
        [STACK; 4],
        [STACK + 4; 4],
        [ERROR, ERRNO, ERROR, ERRNO],
    ] {
        let mut p = fixture();
        let args = [ROOT, outputs[0], outputs[1], outputs[2], outputs[3]];
        let mut expected = prepare(&mut p, API, &args);
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (0, 1));
        expected.eip = if outputs == [STACK; 4] {
            5
        } else {
            0x0040_1000
        };
        expected.set_register(Register32::Esp, STACK + 24);
        expected.set_register(Register32::Eax, 1);
        assert_eq!(p.cpu, expected);
        let mut bytes = std::collections::BTreeMap::new();
        for (address, value) in outputs.into_iter().zip([8_u32, 512, 0, 5]) {
            for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
                bytes.insert(address + u32::try_from(offset).unwrap(), byte);
            }
        }
        for (address, expected) in bytes {
            let mut actual = [0];
            p.memory.read(u64::from(address), &mut actual).unwrap();
            assert_eq!(actual, [expected]);
        }
    }
}

#[test]
fn path_faults_scan_bound_and_actor_validation_preserve_state() {
    let mut p = fixture();
    let mut args = arguments();
    args[0] = 0x5000_0000;
    prepare(&mut p, API, &args);
    fault(&mut p);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
    p.memory
        .map_zeroed(0x5000_0000, 32768, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x5000_0000, &vec![b'a'; 32768]).unwrap();
    unsupported(&mut p, &args);
    p.memory.write(0x5000_0000, b"C:\\\0").unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(values(&p), [8, 512, 0, 5]);
    let fs = p.cpu.fs_base();
    p.cpu.set_fs_base(0x6000_0000);
    unsupported(&mut p, &arguments());
    p.cpu.set_fs_base(fs);
    assert_eq!(p.run(1).api_calls, 1);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, b"C:").unwrap();
    args[0] = 0xffff_fffe;
    prepare(&mut p, API, &args);
    fault(&mut p);
    assert_eq!(values(&p), [8, 512, 0, 5]);
    assert_eq!(word(&p, ERROR), 77);
}

#[test]
fn top_address_output_and_incomplete_or_overflowing_frames_are_atomic() {
    let mut p = fixture();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, &[0x11, 0x22]).unwrap();
    let mut args = arguments();
    args[4] = 0xffff_fffe;
    prepare(&mut p, API, &args);
    fault(&mut p);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
    let mut bytes = [0; 2];
    p.memory.read(0xffff_fffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0x11, 0x22]);
    args = arguments();
    p.memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let frame = [0x0040_1000, args[0], args[1], args[2], args[3], args[4]];
    for (i, value) in frame[..5].iter().enumerate() {
        put(&mut p, 0x6000_0fec + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0x6000_0fec);
    fault(&mut p);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
    p.memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x6000_1000, args[4]);
    assert_eq!(p.run(1).api_calls, 1);
    for address in [OUT, OUT + 4, OUT + 8, OUT + 12] {
        put(&mut p, address, 0xfeed_abba);
    }
    for (i, value) in frame.into_iter().enumerate() {
        put(&mut p, 0xffff_ffe8 + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_ffe8);
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
    );
    assert_eq!(run.api_calls, 0);
    assert_eq!(p.cpu, before);
    assert_eq!(values(&p), [0xfeed_abba; 4]);
}
