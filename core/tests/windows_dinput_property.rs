#[path = "support/dinput_property_cases.rs"]
mod dinput_property_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_keyboard_buffer_setting_matches_across_budgets() {
    dinput_property_cases::imported_keyboard_buffer_setting_across_budgets();
}

use dinput_property_cases::{DATA, HEADER, KEYBOARD};
use ring3_core::execution::{
    Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const SET: u32 = 0x7000_0580;
const ROOT: u32 = 0x7001_7800;
const DEVICE: u32 = 0x7001_7900;
const STACK: u32 = 0x1000_ef00;
const CODE: u32 = 0x0040_1000;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    words(p, STACK, &[CODE]);
    words(p, STACK + 4, args);
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let cleanup = if api == 0x7000_0548 {
        4
    } else {
        (args.len() + 1) * 4
    };
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK + u32::try_from(cleanup).unwrap()
    );
    p.cpu.register(Register32::Eax)
}

fn refused(p: &mut Process32, expected: &ProcessStop) {
    let before = p.cpu;
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(&run.reason, expected);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

fn denied(p: &mut Process32, args: &[u32]) {
    prepare(p, SET, args);
    refused(p, &ProcessStop::UnsupportedApi { address: SET });
}

fn fault(p: &mut Process32, header: u32) {
    let before = prepare(p, SET, &[DEVICE, 1, header]);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

fn process() -> Process32 {
    let mut p = Process32::load(
        &imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]),
        64,
    )
    .unwrap();
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, HEADER, &[20, 16, 0, 0, 16]);
    p.memory.write(u64::from(DATA + 64), &KEYBOARD).unwrap();
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, DATA, 0]), 0);
    assert_eq!(call(&mut p, 0x7000_0568, &[ROOT, DATA + 64, DATA, 0]), 0);
    p
}

#[test]
fn buffer_setting_needs_no_format_window_or_teb_and_accepts_read_only_headers() {
    let mut p = process();
    let windows = p.window_snapshots();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, 0x7000_0564, &[ROOT]), 0);
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7ffd_e040, &[88]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for value in [0, 1, 16, 1024, 1025, u32::MAX, 0] {
        words(&mut p, HEADER, &[20, 16, 0, 0, value]);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(call(&mut p, SET, &[DEVICE, 1, HEADER]), 0);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, 0]), 0x8007_0057);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7ffd_e040, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn property_identity_and_header_errors_preserve_device_lifetime() {
    let mut p = process();
    for property in [0, 2, 0xffff, 0x10001, HEADER, u32::MAX] {
        denied(&mut p, &[DEVICE, property, 0x5000_0000]);
    }
    for header in [
        [0, 16, 0, 0, 16],
        [19, 16, 0, 0, 16],
        [21, 16, 0, 0, 16],
        [20, 0, 0, 0, 16],
        [20, 15, 0, 0, 16],
        [20, 17, 0, 0, 16],
        [20, 16, 1, 0, 16],
    ] {
        words(&mut p, HEADER, &header);
        assert_eq!(call(&mut p, SET, &[DEVICE, 1, HEADER]), 0x8007_0057);
    }
    for how in [1, 2, 3, 4, u32::MAX] {
        words(&mut p, HEADER, &[20, 16, 1, how, 16]);
        denied(&mut p, &[DEVICE, 1, HEADER]);
    }
    words(&mut p, HEADER, &[20, 16, 0, 0, 16]);
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, HEADER]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
    for receiver in [0, ROOT, DEVICE, DEVICE + 1, DEVICE + 4, u32::MAX] {
        denied(&mut p, &[receiver, 1, 0x5000_0000]);
    }
}

#[test]
fn full_property_span_is_checked_before_publication_and_faults_can_be_repaired() {
    let mut p = process();
    words(&mut p, 0x3000_0ff8, &[20, 15]);
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, 0x3000_0ff8]), 0x8007_0057);
    words(&mut p, 0x3000_0ff8, &[19, 16]);
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, 0x3000_0ff8]), 0x8007_0057);
    words(&mut p, 0x3000_0ff8, &[20, 16]);
    for header in [0x3000_0ff8, 0x3000_0ffd, 0x5000_0000] {
        fault(&mut p, header);
    }
    p.memory
        .map_zeroed(0x3000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x3000_1000, &[0, 0, 32]);
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, 0x3000_0ff8]), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_ffed, &[20, 16]);
    prepare(&mut p, SET, &[DEVICE, 1, 0xffff_ffed]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    words(&mut p, 0xffff_ffec, &[20, 16, 0, 0, 64]);
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, 0xffff_ffec]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
}

#[test]
fn zero_incomplete_and_overflowing_frames_stop_before_setting() {
    let mut p = process();
    let before = prepare(&mut p, SET, &[DEVICE, 1, HEADER]);
    let run = p.run(0);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    words(&mut p, 0x1000_fffc, &[CODE]);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff0, &[CODE, DEVICE, 1, HEADER]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[DEVICE, 1, HEADER]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
}

#[test]
fn foreign_and_scheduled_child_property_calls_stop_before_guest_reads() {
    let mut p = process();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = SET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    let child = call(&mut p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    p.cpu.eip = SET;
    p.cpu.set_register(Register32::Esp, 0x5000_0000);
    refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
}
