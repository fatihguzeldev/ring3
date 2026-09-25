use super::dinput_format_cases;
use super::imported_executable;

#[test]
fn imported_standard_keyboard_setup_matches_across_budgets() {
    dinput_format_cases::imported_standard_keyboard_format_across_budgets();
}

use dinput_format_cases::{DATA, FORMAT, KEY, KEY_GUID, OBJECTS, standard};
use ring3_core::execution::{
    Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const SET: u32 = 0x7000_0578;
const STACK: u32 = 0x1000_ef00;
const CODE: u32 = 0x0040_1000;

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(CODE)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
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

fn denied(p: &mut Process32, receiver: u32, format: u32) {
    prepare(p, SET, &[receiver, format]);
    refused(p, &ProcessStop::UnsupportedApi { address: SET });
}

fn fault(p: &mut Process32, receiver: u32, format: u32) {
    prepare(p, SET, &[receiver, format]);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

fn process() -> (Process32, u32, u32) {
    let mut p = Process32::load(
        &imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]),
        96,
    )
    .unwrap();
    p.memory
        .map_zeroed(0x3000_0000, 12288, Permissions::READ_WRITE)
        .unwrap();
    standard(&mut p.memory, false);
    p.memory
        .write(
            u64::from(DATA + 64),
            &[
                0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0, 0,
            ],
        )
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, DATA, 0]), 0);
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    (p, 0x7001_7800, 0x7001_7900)
}

#[test]
fn setup_uses_read_only_unaligned_sources_and_no_error_storage_or_extra_pages() {
    let (mut p, root, device) = process();
    p.memory.write(u64::from(KEY + 32), &KEY_GUID).unwrap();
    for index in (0..256).step_by(2) {
        put(&mut p, OBJECTS + index * 16, KEY + 32);
    }
    p.memory
        .protect(0x3000_0000, 12288, Permissions::READ)
        .unwrap();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, 0x7000_0564, &[root]), 0);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, SET, &[device, 0]), 0x8000_4003);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7ffd_e040, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn header_validation_precedes_unneeded_reads_and_bounds_untrusted_counts() {
    let (mut p, _, device) = process();
    put(&mut p, 0x1000_fffc, 0);
    assert_eq!(call(&mut p, SET, &[device, 0x1000_fffc]), 0x8007_0057);
    put(&mut p, 0x1000_fffc, 24);
    fault(&mut p, device, 0x1000_fffc);
    put(&mut p, 0x1000_fff8, 24);
    put(&mut p, 0x1000_fffc, 0);
    assert_eq!(call(&mut p, SET, &[device, 0x1000_fff8]), 0x8007_0057);
    put(&mut p, 0x1000_fffc, 16);
    fault(&mut p, device, 0x1000_fff8);
    for (offset, value) in [
        (8, 1),
        (8, 3),
        (12, 255),
        (12, u32::MAX),
        (16, 0),
        (16, 255),
        (16, 257),
        (16, u32::MAX),
    ] {
        standard(&mut p.memory, false);
        put(&mut p, FORMAT + offset, value);
        put(&mut p, FORMAT + 20, 0x5000_0000);
        denied(&mut p, device, FORMAT);
    }
    standard(&mut p.memory, false);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
}

#[test]
fn descriptor_profiles_and_duplicates_are_refused_at_either_end() {
    let (mut p, _, device) = process();
    for index in [0, 255] {
        for (field, value) in [
            (0, 0),
            (4, 256),
            (8, 0x0000_000c),
            (8, 0x8000_0003),
            (12, 1),
        ] {
            standard(&mut p.memory, false);
            put(&mut p, OBJECTS + index * 16 + field, value);
            denied(&mut p, device, FORMAT);
        }
    }
    for reverse in [false, true] {
        standard(&mut p.memory, reverse);
        let mut first = [0; 16];
        p.memory.read(u64::from(OBJECTS), &mut first).unwrap();
        p.memory
            .write(u64::from(OBJECTS + 255 * 16), &first)
            .unwrap();
        denied(&mut p, device, FORMAT);
    }
    standard(&mut p.memory, false);
    p.memory.write(u64::from(KEY), &[0; 16]).unwrap();
    denied(&mut p, device, FORMAT);
    standard(&mut p.memory, false);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
}

#[test]
fn header_array_and_guid_faults_precede_commit_and_bad_guid_precedes_bad_tuple() {
    let (mut p, _, device) = process();
    for format in [0x5000_0000, u32::MAX - 1] {
        fault(&mut p, device, format);
    }
    for array in [0, 0x5000_0000, 0x3000_2002, u32::MAX - 4094] {
        standard(&mut p.memory, false);
        put(&mut p, FORMAT + 20, array);
        fault(&mut p, device, FORMAT);
    }
    for index in [0, 255] {
        for guid in [0x5000_0000, 0x1000_fff8, u32::MAX - 7] {
            standard(&mut p.memory, false);
            put(&mut p, OBJECTS + index * 16, guid);
            put(&mut p, OBJECTS + index * 16 + 12, 1);
            fault(&mut p, device, FORMAT);
        }
    }
    standard(&mut p.memory, false);
    p.memory
        .protect(0x3000_2000, 4096, Permissions::NONE)
        .unwrap();
    fault(&mut p, device, FORMAT);
    p.memory
        .protect(0x3000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
}

#[test]
fn receiver_identity_and_checked_frames_precede_format_parsing() {
    let (mut p, root, device) = process();
    for receiver in [0, root, device + 1, device + 4, u32::MAX] {
        denied(&mut p, receiver, 0);
    }
    let before = prepare(&mut p, SET, &[device, FORMAT]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (offset, value) in [(0, CODE), (4, device), (8, FORMAT)] {
        put(&mut p, 0xffff_fff4 + offset, value);
    }
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
    denied(&mut p, device, FORMAT);
}

#[test]
fn foreign_and_scheduled_child_identity_refuse_before_reading_frames() {
    let (mut p, _, _) = process();
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
