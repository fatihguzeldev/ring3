#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_window_query_cases.rs"]
mod thread_window_query_cases;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[test]
fn scheduled_children_read_the_same_window_metadata_as_the_primary() {
    thread_window_query_cases::verify();
}

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};
use thread_window_query_cases::{CHILD, DATA, HANDLE, call, prepare, put, queries, scheduled};

fn stopped(p: &mut Process32, before: Cpu32) -> ProcessStop {
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    run.reason
}

#[test]
fn query_frames_are_read_but_stateful_child_gui_remains_guarded_before_frame_reads() {
    let mut p = scheduled();
    let windows = p.window_snapshots();
    let pages = p.memory.mapped_pages();
    for (offset, args) in queries() {
        let before = prepare(&mut p, offset, &args);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        p.cpu.set_register(Register32::Esp, 0);
        let before = p.cpu;
        assert!(matches!(
            stopped(&mut p, before),
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    for offset in [
        0x2a8, 0x2ac, 0x2c0, 0x430, 0x464, 0x2c4, 0x434, 0x5e0, 12, 0xa0,
    ] {
        p.cpu.eip = 0x7000_0000 + offset;
        p.cpu.set_register(Register32::Esp, 0);
        let before = p.cpu;
        assert_eq!(
            stopped(&mut p, before),
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
    }
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn rectangle_output_faults_preflight_all_sixteen_bytes_and_retry_without_partial_writes() {
    let mut p = scheduled();
    let windows = p.window_snapshots();
    let output = 0x0040_2ff8;
    p.memory.write(u64::from(output), &[0x55; 8]).unwrap();
    for offset in [0x2b0, 0x2b4] {
        let before = prepare(&mut p, offset, &[HANDLE, output]);
        for _ in 0..2 {
            assert!(matches!(
                stopped(&mut p, before),
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            let mut bytes = [0; 8];
            p.memory.read(u64::from(output), &mut bytes).unwrap();
            assert_eq!(bytes, [0x55; 8]);
        }
    }
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, 0x2b0, &[HANDLE, output]), 1);
    let mut bytes = [0; 16];
    p.memory.read(u64::from(output), &mut bytes).unwrap();
    assert_eq!(
        bytes,
        windows[0]
            .rectangle
            .map(i32::to_le_bytes)
            .concat()
            .as_slice()
    );
    assert_eq!(p.window_snapshots(), windows);
}

#[test]
fn child_query_errors_and_unsupported_profiles_preserve_primary_window_state() {
    let mut p = scheduled();
    let windows = p.window_snapshots();
    put(&mut p, 0x7ffd_e034, &[77]);
    for (offset, args) in [
        (0x2b8, vec![0xdead_beef]),
        (0x2b0, vec![0xdead_beef, 0]),
        (0x2b4, vec![0xdead_beef, 0]),
        (0x2bc, vec![0xdead_beef, (-16_i32).cast_unsigned()]),
        (0x454, vec![0xdead_beef]),
        (0x458, vec![0xdead_beef, 2]),
    ] {
        put(&mut p, CHILD + 0x34, &[88]);
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ)
            .unwrap();
        let before = prepare(&mut p, offset, &args);
        assert!(matches!(
            stopped(&mut p, before),
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        p.memory
            .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(p.last_error().unwrap(), 88);
        assert_eq!(call(&mut p, offset, &args), 0);
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    for (offset, args) in [
        (0x2bc, vec![HANDLE, 0]),
        (0x2bc, vec![1, (-4_i32).cast_unsigned()]),
        (0x458, vec![HANDLE, 6]),
        (0x26c, vec![7, 0]),
    ] {
        let before = prepare(&mut p, offset, &args);
        assert_eq!(
            stopped(&mut p, before),
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    let before = prepare(&mut p, 0x26c, &[0, 0xdead_beef]);
    assert!(matches!(
        stopped(&mut p, before),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory.write(u64::from(DATA), b"missing\0").unwrap();
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x2b8, &[HANDLE]), 0);
    assert_eq!(call(&mut p, 0x270, &[0xdead_beef]), 0);
    assert_eq!(call(&mut p, 0x438, &[HANDLE, 42]), 0);
    assert_eq!(call(&mut p, 0x26c, &[0, DATA]), 0);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 1400);
    let mut primary_error = [0; 4];
    p.memory.read(0x7ffd_e034, &mut primary_error).unwrap();
    assert_eq!(primary_error, 77_u32.to_le_bytes());
    assert_eq!(p.window_snapshots(), windows);
}
