#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/type_name_executable.rs"]
mod type_name_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_016c;
const THIS: u32 = 0x0040_2180;
const STACK: u32 = 0x1000_ff00;

fn process(limit: u32) -> Process32 {
    Process32::load(&type_name_executable::pe32(), limit).unwrap()
}

fn read(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn prepare(p: &mut Process32, this: u32) {
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Ecx, this);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
}

fn call(p: &mut Process32, this: u32) -> u32 {
    prepare(p, this);
    let mut expected = p.cpu;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let pointer = p.cpu.register(Register32::Eax);
    expected.set_register(Register32::Eax, pointer);
    expected.set_register(Register32::Esp, STACK + 4);
    expected.eip = 0x0040_1000;
    assert_eq!(p.cpu, expected);
    pointer
}

#[test]
fn imported_thiscall_caches_the_same_name_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = process(32);
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
        assert_eq!(counts, (5, 2));
        let pointer = p.cpu.register(Register32::Eax);
        assert_eq!(pointer, p.cpu.register(Register32::Esi));
        assert_eq!(read(&p, THIS + 4, 4), pointer.to_le_bytes());
        assert_eq!(read(&p, pointer, 22), b"class engine::Widget\0\0");
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn kinds_and_namespace_order_preserve_raw_bytes_errors_and_cached_pointer_identity() {
    for (raw, expected) in [
        (".?AVSample@@", "class Sample"),
        (".?AUNode@inner@outer@@", "struct outer::inner::Node"),
        (".?AT_Value2@@", "union _Value2"),
        (".?AW4Mode@engine@@", "enum engine::Mode"),
    ] {
        let mut p = process(32);
        let raw = format!("{raw}\0");
        p.memory.write(u64::from(THIS + 8), raw.as_bytes()).unwrap();
        p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
        p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
        for page in [0x7000_2000, 0x7ffd_e000] {
            p.memory.protect(page, 4096, Permissions::READ).unwrap();
        }
        let pages = p.memory.mapped_pages();
        prepare(&mut p, THIS);
        let before = p.cpu;
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        let pointer = call(&mut p, THIS);
        assert_eq!(pointer, 0x2000_0000);
        assert_eq!(
            read(&p, pointer, expected.len() + 1),
            format!("{expected}\0").as_bytes()
        );
        assert_eq!(read(&p, THIS + 8, raw.len()), raw.as_bytes());
        assert_eq!(read(&p, THIS + 4, 4), pointer.to_le_bytes());
        p.memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(call(&mut p, THIS), pointer);
        assert_eq!(p.memory.mapped_pages(), pages + 1);
        assert_eq!(read(&p, 0x7000_2020, 4), 123_u32.to_le_bytes());
        assert_eq!(p.last_error().unwrap(), 77);
        assert!(p.memory.fetch(u64::from(pointer), &mut [0]).is_err());
    }
}

#[test]
fn unsupported_names_leave_cache_heap_and_cpu_unchanged() {
    for raw in [
        "",
        ".H",
        ".?AV@@",
        ".?AV9Bad@@",
        ".?AVName@",
        ".?AVName@@tail",
        ".?AVName@@@",
        ".?AVName@0@@",
        ".?AV?$Vector@H@@",
        ".?AVName space@@",
        "?AVName@@",
        ".?BVName@@",
        ".?AW3Enum@@",
        ".?AVNáme@@",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([format!(".?AV{}@@", vec!["Part"; 33].join("@"))])
    {
        let mut p = process(32);
        p.memory
            .write(u64::from(THIS + 8), format!("{raw}\0").as_bytes())
            .unwrap();
        prepare(&mut p, THIS);
        let before = p.cpu;
        let pages = p.memory.mapped_pages();
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi { address: API },
            "{raw}"
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(read(&p, THIS + 4, 4), [0; 4]);
    }
}

#[test]
fn cached_pointer_requires_only_the_cache_read_even_at_the_last_guest_word() {
    let mut p = process(32);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(0xffff_fffc, &0xdead_beef_u32.to_le_bytes())
        .unwrap();
    p.memory
        .protect(0xffff_f000, 4096, Permissions::READ)
        .unwrap();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, 0xffff_fff8), 0xdead_beef);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn split_cache_write_is_atomic_and_last_address_terminator_is_not_overread() {
    let mut p = process(32);
    p.memory
        .map_zeroed(0x6000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_1002, b".?AVSample@@\0").unwrap();
    p.memory
        .protect(0x6000_1000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0x6000_0ffa);
    let before = p.cpu;
    let pages = p.memory.mapped_pages();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, 0x6000_0ffe, 4), [0; 4]);
    p.memory
        .protect(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, 0x6000_0ffa), 0x2000_0000);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let raw = b".?AVEnd@@\0";
    let source = u32::MAX - u32::try_from(raw.len() - 1).unwrap();
    p.memory.write(u64::from(source), raw).unwrap();
    let pointer = call(&mut p, source - 8);
    assert_eq!(read(&p, pointer, 10), b"class End\0");
}

#[test]
fn source_cache_and_frame_faults_are_atomic_and_retryable() {
    for this in [0, 0x0040_2ff4, 0xffff_fff8, 0xffff_fffb, u32::MAX] {
        let mut p = process(32);
        p.memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        if this == 0x0040_2ff4 {
            p.memory.write(0x0040_2ffc, b".?AV").unwrap();
        }
        prepare(&mut p, this);
        let before = p.cpu;
        let pages = p.memory.mapped_pages();
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(call(&mut p, THIS), 0x2000_0000);
    }
    for readonly_cache in [false, true] {
        let mut p = process(32);
        prepare(&mut p, THIS);
        if readonly_cache {
            p.memory
                .protect(0x0040_2000, 4096, Permissions::READ)
                .unwrap();
        } else {
            p.cpu.set_register(Register32::Esp, 0x6000_0000);
        }
        let before = p.cpu;
        let pages = p.memory.mapped_pages();
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(read(&p, THIS + 4, 4), [0; 4]);
        p.memory
            .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(call(&mut p, THIS), 0x2000_0000);
    }
}

#[test]
fn exhaustion_sets_errno_and_faulting_error_write_never_leaks_or_caches() {
    let mut p = process(26);
    let first = call(&mut p, THIS);
    p.memory.write(u64::from(THIS + 4), &[0; 4]).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, THIS), 0);
    assert_eq!(read(&p, 0x7000_2020, 4), 12_u32.to_le_bytes());
    assert_eq!(read(&p, THIS + 4, 4), [0; 4]);
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    prepare(&mut p, THIS);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, THIS + 4, 4), [0; 4]);
    prepare(&mut p, THIS);
    p.cpu.eip = 0x7000_0120;
    p.memory
        .write(u64::from(STACK + 4), &first.to_le_bytes())
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(call(&mut p, THIS), 0x2000_0000);
}

#[test]
fn exhaustion_error_write_obeys_guest_aliases_including_the_cache_slot() {
    let mut p = process(25);
    p.memory.write(0x7000_2020, &[0; 4]).unwrap();
    p.memory.write(0x7000_2024, b".?AVSample@@\0").unwrap();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, 0x7000_201c), 0);
    assert_eq!(read(&p, 0x7000_2020, 4), 12_u32.to_le_bytes());
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, 0x7000_201c), 12);
}

#[test]
fn scan_and_scope_limits_include_the_last_valid_byte_and_component() {
    let mut p = process(32);
    let mut raw = b".?AV".to_vec();
    raw.extend(vec![b'a'; 1017]);
    raw.extend_from_slice(b"@@\0");
    assert_eq!(raw.len(), 1024);
    p.memory.write(u64::from(THIS + 8), &raw).unwrap();
    let pointer = call(&mut p, THIS);
    assert_eq!(read(&p, pointer, 6), b"class ");
    assert_eq!(read(&p, pointer + 6, 1018), [&raw[4..1021], &[0]].concat());
    p.memory.write(u64::from(THIS + 4), &[0; 4]).unwrap();
    p.memory.write(u64::from(THIS + 8 + 1023), b"a").unwrap();
    prepare(&mut p, THIS);
    let before = p.cpu;
    let pages = p.memory.mapped_pages();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    let raw = format!(".?AU{}@@\0", vec!["Part"; 32].join("@"));
    p.memory.write(u64::from(THIS + 8), raw.as_bytes()).unwrap();
    let pointer = call(&mut p, THIS);
    let expected = format!("struct {}\0", vec!["Part"; 32].join("::"));
    assert_eq!(read(&p, pointer, expected.len()), expected.as_bytes());
}

#[test]
fn cache_can_alias_the_saved_return_address_and_is_reread_after_publication() {
    let mut p = process(32);
    let this = STACK - 4;
    prepare(&mut p, this);
    p.memory.write(u64::from(STACK), &[0; 4]).unwrap();
    p.memory
        .write(u64::from(STACK + 4), b".?AVSample@@\0")
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0x2000_0000);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 4);
    assert_eq!(p.cpu.register(Register32::Eax), 0x2000_0000);
    assert_eq!(read(&p, STACK, 4), 0x2000_0000_u32.to_le_bytes());
}
