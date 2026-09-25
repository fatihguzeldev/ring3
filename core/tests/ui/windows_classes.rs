use super::window_class_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_window_class_lifetime_runs_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = Process32::load(&window_class_executable::pe32(), 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (11, 3));
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Ebx), 0xc000);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut actual = [0; 40];
        p.memory.read(0x0040_2280, &mut actual).unwrap();
        assert_eq!(actual[..4], 3_u32.to_le_bytes());
        assert_eq!(actual[36..], 0x0040_2180_u32.to_le_bytes());
    }
}

#[test]
fn classes_share_user_atoms_with_pinned_messages_and_clipboard_formats() {
    for pin in [0x7000_0094, 0x7000_00c8] {
        let mut p = load();
        let other = 0x0040_2300;
        p.memory.write(u64::from(other), b"other\0").unwrap();
        assert_eq!(call(&mut p, pin, &[other]), 0xc000);
        assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc001);
        assert_eq!(call(&mut p, pin, &[NAME]), 0xc001);
        assert_eq!(call(&mut p, REMOVE, &[0xc001, INSTANCE]), 1);
        assert_eq!(call(&mut p, QUERY, &[INSTANCE, NAME, OUTPUT]), 0);
        assert_eq!(call(&mut p, pin, &[NAME]), 0xc001);
        assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc001);
        for api in [0x7000_0094, 0x7000_00c8] {
            assert_eq!(call(&mut p, api, &[NAME]), 0xc001);
            assert_eq!(call(&mut p, api, &[other]), 0xc000);
        }
        p.memory.write(u64::from(NAME), b"transient\0").unwrap();
        assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc002);
        assert_eq!(call(&mut p, REMOVE, &[NAME, INSTANCE]), 1);
        assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc003);
        assert_eq!(call(&mut p, QUERY, &[INSTANCE, 0xc002, OUTPUT]), 0);
    }
}

const QUERY: u32 = 0x7000_0260;
const REGISTER: u32 = 0x7000_0264;
const REMOVE: u32 = 0x7000_0268;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2180;
const RECORD: u32 = 0x0040_21c0;
const OUTPUT: u32 = 0x0040_2280;
const INSTANCE: u32 = 0x0040_0000;
const ERROR: u32 = 0x7ffd_e034;

fn load() -> Process32 {
    Process32::load(&window_class_executable::pe32(), 64).unwrap()
}
fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (i, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}
fn bytes(p: &Process32, address: u32, count: usize) -> Vec<u8> {
    let mut out = vec![0; count];
    p.memory.read(u64::from(address), &mut out).unwrap();
    out
}
fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Esp, STACK);
    words(p, STACK, &[0x0040_1000]);
    words(p, STACK + 4, args);
    p.cpu
}
fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn snapshot_names_atoms_instances_and_processes_have_independent_lifetimes() {
    let mut p = load();
    let mut other = load();
    let original = bytes(&p, RECORD, 40);
    words(&mut p, ERROR, &[77]);
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, QUERY, &[INSTANCE, NAME, OUTPUT]), 0);
    assert_eq!(p.last_error().unwrap(), 1411);
    words(&mut p, ERROR, &[77]);
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory.write(u64::from(NAME), b"DEMO\0").unwrap();
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0);
    assert_eq!(p.last_error().unwrap(), 1410);
    assert_eq!(call(&mut p, QUERY, &[INSTANCE, NAME, OUTPUT]), 0xc000);
    assert_eq!(bytes(&p, OUTPUT, 40), original);
    assert_eq!(call(&mut other, QUERY, &[INSTANCE, 0xc000, OUTPUT]), 0);
    assert_eq!(other.last_error().unwrap(), 1411);
    words(&mut p, RECORD + 4, &[0xdead_beef]);
    words(&mut p, RECORD + 16, &[0x7000_0800]);
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
    assert_eq!(call(&mut p, QUERY, &[0x7000_0800, 0xc000, OUTPUT]), 0xc000);
    assert_eq!(bytes(&p, OUTPUT + 4, 4), 0xdead_beef_u32.to_le_bytes());
    assert_eq!(bytes(&p, OUTPUT + 36, 4), 0xc000_u32.to_le_bytes());
    assert_eq!(call(&mut p, REMOVE, &[0xc000, INSTANCE]), 1);
    assert_eq!(call(&mut p, QUERY, &[INSTANCE, NAME, OUTPUT]), 0);
    assert_eq!(call(&mut p, QUERY, &[0x7000_0800, NAME, OUTPUT]), 0xc000);
    p.memory.write(u64::from(NAME), b"xxxx\0").unwrap();
    assert_eq!(call(&mut p, REMOVE, &[0xc000, 0x7000_0800]), 1);
    p.memory.write(u64::from(NAME), b"demo\0").unwrap();
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc001);
    assert_eq!(call(&mut p, REMOVE, &[0xc000, 0x7000_0800]), 0);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn unsupported_record_fields_names_and_system_classes_do_not_register() {
    let mut p = load();
    let record = bytes(&p, RECORD, 40);
    for (index, value) in [
        (0, 0x4000),
        (0, 0x20),
        (1, 0),
        (2, 1),
        (3, 1),
        (4, 0),
        (4, 123),
        (5, 1),
        (6, 1),
        (7, 1),
        (8, 1),
        (9, 0xc000),
    ] {
        p.memory.write(u64::from(RECORD), &record).unwrap();
        words(&mut p, RECORD + index * 4, &[value]);
        let before = prepare(&mut p, REGISTER, &[RECORD]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: REGISTER }
        );
        assert_eq!(p.cpu, before);
    }
    p.memory.write(u64::from(RECORD), &record).unwrap();
    for name in [
        b"\0".as_slice(),
        b"button\0",
        b"Edit\0",
        b"Message\0",
        b"#32770\0",
        b"\x80\0",
    ] {
        p.memory.write(u64::from(NAME), name).unwrap();
        for (api, args) in [
            (REGISTER, vec![RECORD]),
            (QUERY, vec![INSTANCE, NAME, OUTPUT]),
            (REMOVE, vec![NAME, INSTANCE]),
        ] {
            let before = prepare(&mut p, api, &args);
            assert_eq!(
                p.run(1).reason,
                ProcessStop::UnsupportedApi { address: api }
            );
            assert_eq!(p.cpu, before);
        }
    }
    p.memory.write(u64::from(NAME), b"demo\0").unwrap();
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
}

#[test]
fn errors_frames_and_zero_budget_leave_definitions_unchanged() {
    let mut p = load();
    let before = prepare(&mut p, REGISTER, &[RECORD]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for (api, args) in [
        (QUERY, vec![INSTANCE, NAME, OUTPUT]),
        (REMOVE, vec![NAME, INSTANCE]),
    ] {
        let before = prepare(&mut p, api, &args);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
    let before = prepare(&mut p, REGISTER, &[RECORD]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    for (api, args) in [
        (REGISTER, vec![RECORD]),
        (QUERY, vec![INSTANCE, NAME, OUTPUT]),
        (REMOVE, vec![NAME, INSTANCE]),
    ] {
        prepare(&mut p, api, &args);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, QUERY, &[INSTANCE, NAME, OUTPUT]), 0xc000);
    assert_eq!(call(&mut p, REMOVE, &[NAME, INSTANCE]), 1);
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc001);
}

#[test]
fn query_snapshot_and_checked_write_handle_aliases_and_page_boundaries() {
    let mut p = load();
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
    for output in [RECORD, NAME, STACK, ERROR, 0x7000_2020] {
        p.memory.write(u64::from(NAME), b"demo\0").unwrap();
        let mut expected = prepare(&mut p, QUERY, &[INSTANCE, NAME, output]);
        assert_eq!(p.run(1).api_calls, 1);
        expected.eip = if output == STACK { 3 } else { 0x0040_1000 };
        expected.set_register(Register32::Esp, STACK + 16);
        expected.set_register(Register32::Eax, 0xc000);
        assert_eq!(p.cpu, expected);
        assert_eq!(bytes(&p, output + 36, 4), NAME.to_le_bytes());
    }
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_0fff, &[0xa5]).unwrap();
    let before = prepare(&mut p, QUERY, &[INSTANCE, 0xc000, 0x3000_0fff]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, 0x3000_0fff, 1), [0xa5]);
    p.memory
        .protect(
            0x3000_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(
        call(&mut p, QUERY, &[INSTANCE, 0xc000, 0x3000_0001]),
        0xc000
    );
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(bytes(&p, 0x3000_0025, 4), 0xc000_u32.to_le_bytes());
    let before = prepare(&mut p, QUERY, &[INSTANCE, 0xc000, 0x3000_0001]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn bounded_names_survive_source_permissions_and_full_records_respect_guest_end() {
    let mut p = load();
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let mut name = vec![b'a'; 255];
    name.push(0);
    p.memory.write(0x3000_0000, &name).unwrap();
    words(&mut p, RECORD + 36, &[0x3000_0000]);
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(
        call(&mut p, QUERY, &[INSTANCE, 0xc000, 0xffff_ffd8]),
        0xc000
    );
    let old = bytes(&p, 0xffff_ffd8, 40);
    for (api, args) in [
        (REGISTER, vec![RECORD]),
        (REGISTER, vec![0xffff_ffe0]),
        (QUERY, vec![INSTANCE, 0xc000, 0xffff_ffd9]),
    ] {
        let before = prepare(&mut p, api, &args);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, 0xffff_ffd8, 40), old);
    }
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_00ff, b"a\0").unwrap();
    let before = prepare(&mut p, REGISTER, &[RECORD]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: REGISTER }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, REMOVE, &[0xc000, INSTANCE]), 1);
}

#[test]
fn live_capacity_recovers_without_recycling_atoms() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    for index in 0..4096 {
        let name = format!("class{index}\0");
        p.memory.write(u64::from(NAME), name.as_bytes()).unwrap();
        assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xc000 + index);
    }
    p.memory.write(u64::from(NAME), b"extra\0").unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, REGISTER, &[RECORD]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(call(&mut p, REMOVE, &[0xc000, INSTANCE]), 1);
    assert_eq!(call(&mut p, REGISTER, &[RECORD]), 0xd000);
    assert_eq!(call(&mut p, QUERY, &[INSTANCE, 0xc000, OUTPUT]), 0);
    assert_eq!(p.last_error().unwrap(), 1411);
    assert_eq!(p.memory.mapped_pages(), pages);
}
