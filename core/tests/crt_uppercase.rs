#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01a8;
const SET_CODE_PAGE: u32 = 0x7000_013c;
const STACK: u32 = 0x1000_ff00;
const BUFFER: u32 = 0x0040_2180;
const ERRNO: u32 = 0x7000_2020;

fn process() -> Process32 {
    let code = [
        0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc,
    ];
    Process32::load(
        &imported_executable::pe32(&code, "mSvCrT.dll", &["_strupr"]),
        128,
    )
    .unwrap()
}

fn bytes(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn prepare(p: &mut Process32, api: u32, argument: u32) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, argument].into_iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

fn failure(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let result = p.run(1);
    if unsupported {
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    } else {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn imported_uppercase_changes_the_guest_string_and_returns_its_pointer() {
    for budget in [1, 20] {
        let mut p = process();
        p.memory
            .write(u64::from(BUFFER), b"Mixed-path_19\0")
            .unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 20);
        }
        assert_eq!(counts, (4, 1));
        assert_eq!(p.cpu.register(Register32::Eax), BUFFER);
        assert_eq!(bytes(&p, BUFFER, 14), b"MIXED-PATH_19\0");
    }
}

#[test]
fn c_locale_uppercases_ascii_and_preserves_error_state_and_other_bytes() {
    let mut p = process();
    p.memory
        .write(u64::from(ERRNO), &88_u32.to_le_bytes())
        .unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();

    let input = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ09-_[\xc4\xe4\xff\0tail";
    let expected = b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZ09-_[\xc4\xe4\xff\0tail";
    for code_page in [0, 1252] {
        let before = prepare(&mut p, SET_CODE_PAGE, code_page);
        success(&mut p, before, 0);
        p.memory.write(u64::from(BUFFER), input).unwrap();
        let before = prepare(&mut p, API, BUFFER);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, BUFFER);
        assert_eq!(bytes(&p, BUFFER, input.len()), expected);
        assert_eq!(
            u32::from_le_bytes(bytes(&p, ERRNO, 4).try_into().unwrap()),
            88
        );
        assert_eq!(p.last_error().unwrap(), 77);
    }
}

#[test]
fn null_fault_and_unterminated_inputs_do_not_partially_change_memory() {
    let mut p = process();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, 0);
    failure(&mut p, false);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = p.cpu;
    success(&mut p, before, 0);
    assert_eq!(
        u32::from_le_bytes(bytes(&p, ERRNO, 4).try_into().unwrap()),
        22
    );

    p.memory
        .map_zeroed(0x2000_0000, 65_536, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'a'; 65_536]).unwrap();
    prepare(&mut p, API, 0x2000_0000);
    failure(&mut p, true);
    assert_eq!(bytes(&p, 0x2000_0000, 65_536), vec![b'a'; 65_536]);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    let before = prepare(&mut p, API, 0x2000_0000);
    success(&mut p, before, 0x2000_0000);
    assert_eq!(bytes(&p, 0x2000_0000, 65_535), vec![b'A'; 65_535]);

    p.memory.write(u64::from(BUFFER), b"lowercase\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, BUFFER);
    failure(&mut p, false);
    assert_eq!(bytes(&p, BUFFER, 10), b"lowercase\0");
}

#[test]
fn read_and_argument_faults_preserve_cpu_and_prefix_bytes() {
    let mut p = process();
    p.memory
        .map_zeroed(0x3000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_0fff, b"ab\0").unwrap();
    p.memory
        .protect(0x3000_1000, 4096, Permissions::NONE)
        .unwrap();
    prepare(&mut p, API, 0x3000_0fff);
    failure(&mut p, false);
    assert_eq!(bytes(&p, 0x3000_0fff, 1), b"a");

    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(u32::MAX), b"a").unwrap();
    prepare(&mut p, API, u32::MAX);
    failure(&mut p, false);
    assert_eq!(bytes(&p, u32::MAX, 1), b"a");

    prepare(&mut p, API, BUFFER);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    failure(&mut p, false);
}
