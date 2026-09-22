#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/registry_executable.rs"]
mod registry_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_key_lifetime_runs_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = Process32::load(&registry_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 40);
        }
        assert_eq!(counts, (22, 3));
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7600_0004);
        assert_eq!(p.cpu.register(Register32::Esi), 0x7600_0008);
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

const OPEN: u32 = 0x7000_0278;
const CREATE: u32 = 0x7000_027c;
const CLOSE: u32 = 0x7000_0280;
const ROOT: u32 = 0x8000_0001;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2180;
const OUT: u32 = 0x0040_2300;
const DISPOSITION: u32 = OUT + 4;

fn load() -> Process32 {
    Process32::load(&registry_executable::pe32(), 32).unwrap()
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

fn name(p: &mut Process32, value: &[u8]) {
    p.memory.write(u64::from(NAME), value).unwrap();
    p.memory
        .write(u64::from(NAME) + value.len() as u64, &[0])
        .unwrap();
}

fn prepare(p: &mut Process32, address: u32, args: &[u32]) -> ring3_core::execution::Cpu32 {
    p.cpu.eip = address;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    put(p, STACK, 0x0040_1000);
    for (index, &arg) in args.iter().enumerate() {
        put(p, STACK + 4 + u32::try_from(index).unwrap() * 4, arg);
    }
    p.cpu
}

fn call(p: &mut Process32, address: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, address, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = word(p, STACK);
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, expected);
    value
}

fn open(p: &mut Process32, handle: u32) -> u32 {
    call(p, OPEN, &[handle, NAME, 0, 0x2001f, OUT])
}

fn create(p: &mut Process32, handle: u32) -> u32 {
    call(
        p,
        CREATE,
        &[handle, NAME, 0, 0, 0, 0x2001f, 0, OUT, DISPOSITION],
    )
}

#[test]
fn keys_survive_handles_with_case_insensitive_relative_identity() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    name(&mut p, b"sOfTwArE");
    assert_eq!(open(&mut p, ROOT), 0);
    let software = word(&p, OUT);
    name(&mut p, b"Example\\Settings");
    assert_eq!(create(&mut p, software), 0);
    let settings = word(&p, OUT);
    assert_eq!(word(&p, DISPOSITION), 1);
    assert_eq!(call(&mut p, CLOSE, &[settings]), 0);
    assert_eq!(call(&mut p, CLOSE, &[settings]), 6);
    assert_eq!(call(&mut p, CLOSE, &[software]), 0);
    name(&mut p, b"SOFTWARE\\EXAMPLE");
    assert_eq!(open(&mut p, ROOT), 0);
    let parent = word(&p, OUT);
    name(&mut p, b"settings");
    assert_eq!(create(&mut p, parent), 0);
    assert_eq!(word(&p, DISPOSITION), 2);
    assert!(word(&p, OUT) > settings);
    assert_eq!(p.memory.mapped_pages(), pages);
    let mut other = load();
    name(&mut other, b"Software\\Example");
    assert_eq!(open(&mut other, ROOT), 2);
    assert_eq!(open(&mut other, software), 6);
}

#[test]
fn predefined_and_empty_subkeys_obey_handle_lifetime() {
    let mut p = load();
    for root in [ROOT, 0x8000_0002] {
        for pointer in [0, NAME] {
            name(&mut p, b"");
            assert_eq!(call(&mut p, OPEN, &[root, pointer, 0, 0xf003f, OUT]), 0);
            assert_eq!(word(&p, OUT), root);
        }
        assert_eq!(call(&mut p, CLOSE, &[root]), 0);
        name(&mut p, b"");
        assert_eq!(create(&mut p, root), 0);
        let duplicate = word(&p, OUT);
        assert_ne!(duplicate, root);
        assert_eq!(call(&mut p, OPEN, &[duplicate, 0, 0, 1, OUT]), 0);
        let second = word(&p, OUT);
        assert_ne!(second, duplicate);
        assert_eq!(call(&mut p, CLOSE, &[duplicate]), 0);
        name(&mut p, b"software");
        assert_eq!(open(&mut p, second), 0);
    }
}

#[test]
fn missing_invalid_and_machine_root_failures_preserve_outputs_and_errors() {
    let mut p = load();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7000_2020, 88);
    put(&mut p, OUT, 123);
    put(&mut p, DISPOSITION, 456);
    name(&mut p, b"missing\\child");
    assert_eq!(open(&mut p, ROOT), 2);
    assert_eq!(create(&mut p, 0x8000_0002), 5);
    for handle in [
        0,
        1,
        0x7000_0818,
        0x7200_0004,
        0x7400_0004,
        0x7600_0004,
        u32::MAX,
    ] {
        assert_eq!(call(&mut p, OPEN, &[handle, u32::MAX, 0, 0, u32::MAX]), 6);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 6);
    }
    assert_eq!(word(&p, OUT), 123);
    assert_eq!(word(&p, DISPOSITION), 456);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7000_2020), 88);
    name(&mut p, b"Software\\Acme");
    assert_eq!(create(&mut p, 0x8000_0002), 0);
    assert_eq!(open(&mut p, ROOT), 2);
    assert_eq!(open(&mut p, 0x8000_0002), 0);
}

#[test]
fn unsupported_profiles_do_not_allocate_or_mutate() {
    let mut p = load();
    name(&mut p, b"Software");
    let mut cases = vec![
        (OPEN, vec![0x8000_0000, NAME, 0, 1, OUT]),
        (OPEN, vec![ROOT, NAME, 1, 1, OUT]),
        (CREATE, vec![ROOT, 0, 0, 0, 0, 1, 0, OUT, 0]),
    ];
    for mask in [0x100, 0x200, 0x0010_0000, 0x8000_0000, 0x0100_0000] {
        cases.push((OPEN, vec![ROOT, NAME, 0, mask, OUT]));
    }
    for index in [2, 3, 4, 6] {
        let mut args = vec![ROOT, NAME, 0, 0, 0, 1, 0, OUT, 0];
        args[index] = 1;
        cases.push((CREATE, args));
    }
    for (address, args) in cases {
        let before = prepare(&mut p, address, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(open(&mut p, ROOT), 0);
    assert_eq!(word(&p, OUT), 0x7600_0004);
}

#[test]
fn path_bounds_and_literal_components_are_explicit() {
    let mut p = load();
    for value in [
        b"\\a".as_slice(),
        b"a\\",
        b"a\\\\b",
        b"\xff",
        &vec![b'a'; 256],
        (0..33)
            .map(|_| "a")
            .collect::<Vec<_>>()
            .join("\\")
            .as_bytes(),
    ] {
        name(&mut p, value);
        let before = prepare(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, OUT, 0]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: CREATE }
        );
        assert_eq!(p.cpu, before);
    }
    name(&mut p, &vec![b'a'; 255]);
    assert_eq!(create(&mut p, ROOT), 0);
    let long = word(&p, OUT);
    name(&mut p, b"b");
    let before = prepare(&mut p, CREATE, &[long, NAME, 0, 0, 0, 1, 0, OUT, 0]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: CREATE }
    );
    assert_eq!(p.cpu, before);
    for value in [
        b"..\\.\\a/b:c".as_slice(),
        (0..32)
            .map(|_| "a")
            .collect::<Vec<_>>()
            .join("\\")
            .as_bytes(),
    ] {
        name(&mut p, value);
        assert_eq!(create(&mut p, ROOT), 0);
        assert_eq!(open(&mut p, ROOT), 0);
    }
}

#[test]
fn failed_outputs_never_create_keys_or_consume_handles() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    name(&mut p, b"Software\\Uncommitted\\Leaf");
    put(&mut p, OUT, 123);
    for (output, disposition) in [(OUT, u32::MAX), (0xffff_fffe, 0), (0, DISPOSITION)] {
        let before = prepare(
            &mut p,
            CREATE,
            &[ROOT, NAME, 0, 0, 0, 1, 0, output, disposition],
        );
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(run.api_calls, 0);
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, OUT), 123);
        assert_eq!(open(&mut p, ROOT), 2);
    }
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, OUT, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    name(&mut p, b"Software\\Uncommitted");
    assert_eq!(open(&mut p, ROOT), 2);
    name(&mut p, b"Software");
    assert_eq!(open(&mut p, ROOT), 0);
    assert_eq!(word(&p, OUT), 0x7600_0004);
}

#[test]
fn bounded_sources_and_nine_argument_frames_fault_without_mutation() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_ffff, b"a").unwrap();
    for pointer in [0xffff_ffff, 0x0040_3000] {
        let before = prepare(&mut p, CREATE, &[ROOT, pointer, 0, 0, 0, 1, 0, OUT, 0]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    name(&mut p, b"Software\\NeverCreated");
    prepare(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, OUT, 0]);
    let stack = 0x1000_ffdc;
    for (index, value) in [0x0040_1000, ROOT, NAME, 0, 0, 0, 1, 0, OUT]
        .into_iter()
        .enumerate()
    {
        put(&mut p, stack + u32::try_from(index).unwrap() * 4, value);
    }
    p.cpu.set_register(Register32::Esp, stack);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(open(&mut p, ROOT), 2);
    let before = prepare(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, OUT, 0]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    name(&mut p, b"Software");
    assert_eq!(open(&mut p, ROOT), 0);
    assert_eq!(word(&p, OUT), 0x7600_0004);
    p.memory.write(0xffff_fff7, b"software\0").unwrap();
    assert_eq!(call(&mut p, OPEN, &[ROOT, 0xffff_fff7, 0, 1, OUT]), 0);
}

#[test]
fn success_and_lstatus_failures_do_not_touch_error_cells() {
    use ring3_core::execution::Permissions;
    let mut p = load();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7000_2020, 88);
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    name(&mut p, b"Software\\Example");
    assert_eq!(open(&mut p, ROOT), 2);
    assert_eq!(create(&mut p, ROOT), 0);
    let handle = word(&p, OUT);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 0);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 6);
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory
            .protect(page, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7000_2020), 88);
}

#[test]
fn output_aliases_use_snapshotted_names_and_saved_return_reread() {
    let mut p = load();
    name(&mut p, b"Software\\Alias");
    assert_eq!(
        call(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, NAME, 0]),
        0
    );
    let handle = word(&p, NAME);
    assert_eq!(handle, 0x7600_0004);
    name(&mut p, b"software\\alias");
    assert_eq!(open(&mut p, ROOT), 0);
    name(&mut p, b"Other");
    assert_eq!(
        call(&mut p, CREATE, &[ROOT, NAME, 0, 0, 0, 1, 0, OUT, OUT]),
        0
    );
    assert_eq!(word(&p, OUT), 1);
    assert_eq!(call(&mut p, CLOSE, &[0x7600_000c]), 0);
    name(&mut p, b"Software");
    assert_eq!(call(&mut p, OPEN, &[ROOT, NAME, 0, 1, STACK]), 0);
    assert_eq!(p.cpu.eip, 0x7600_0010);
}

#[test]
fn other_predefined_hives_are_explicitly_unsupported_for_all_calls() {
    let mut p = load();
    name(&mut p, b"Software");
    put(&mut p, OUT, 123);
    for root in [
        0x8000_0000,
        0x8000_0003,
        0x8000_0004,
        0x8000_0005,
        0x8000_0006,
        0x8000_0007,
        0x8000_0050,
        0x8000_0060,
    ] {
        for (address, args) in [
            (OPEN, vec![root, NAME, 0, 1, OUT]),
            (CREATE, vec![root, NAME, 0, 0, 0, 1, 0, OUT, DISPOSITION]),
            (CLOSE, vec![root]),
        ] {
            let before = prepare(&mut p, address, &args);
            assert_eq!(p.run(1).reason, ProcessStop::UnsupportedApi { address });
            assert_eq!(p.cpu, before);
            assert_eq!(word(&p, OUT), 123);
        }
    }
    assert_eq!(open(&mut p, ROOT), 0);
    assert_eq!(word(&p, OUT), 0x7600_0004);
}
