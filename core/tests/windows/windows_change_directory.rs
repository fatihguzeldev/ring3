use super::change_directory_executable;
use ring3_core::execution::{
    Cpu32, LoadError, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

#[test]
fn imported_directory_change_and_query_use_process_state() {
    for budget in [1, 50] {
        let mut p = Process32::load(&change_directory_executable::pe32(), 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (7, 2));
        assert_eq!(p.cpu.register(Register32::Ebx), 1);
        assert_eq!(p.cpu.register(Register32::Eax), 3);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 4];
        p.memory.read(0x0040_2280, &mut bytes).unwrap();
        assert_eq!(&bytes, b"C:\\\0");
    }
}
const API: u32 = 0x7000_0230;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x3000_0000;

fn load(current: &[u8], directories: &[&[u8]]) -> Process32 {
    let mut p = Process32::load_with_options(
        &change_directory_executable::pe32(),
        128,
        ProcessOptions {
            current_directory: current,
            directories,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory
        .map_zeroed(u64::from(OUTPUT), PAGE_SIZE * 8, Permissions::READ_WRITE)
        .unwrap();
    p
}

fn prepare(p: &mut Process32, source: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Ebx, 0x1234_5678);
    p.cpu.eflags = 0xced7;
    for (i, word) in [0x0040_1000_u32, source].into_iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn complete(p: &mut Process32, mut before: Cpu32, result: u32, resume: u32) {
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    before.eip = resume;
    before.set_register(Register32::Esp, before.register(Register32::Esp) + 8);
    before.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, before);
}

fn change(p: &mut Process32, input: &[u8], expected: u32) {
    p.memory.write(u64::from(SOURCE), input).unwrap();
    p.memory
        .write(u64::from(SOURCE) + input.len() as u64, &[0])
        .unwrap();
    let before = prepare(p, SOURCE);
    complete(p, before, expected, 0x0040_1000);
}

fn current(p: &mut Process32) -> Vec<u8> {
    prepare(p, 32768);
    p.cpu.eip = 0x7000_0228;
    p.memory
        .write(u64::from(STACK + 8), &OUTPUT.to_le_bytes())
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    let length = usize::try_from(p.cpu.register(Register32::Eax)).unwrap();
    let mut output = vec![0; length + 1];
    p.memory.read(u64::from(OUTPUT), &mut output).unwrap();
    assert_eq!(output.pop(), Some(0));
    output
}

#[test]
fn declared_paths_ancestors_and_relative_transitions_are_owned_and_isolated() {
    let mut declared = b"C:\\Base\\Other\\Deep".to_vec();
    let mut initial = b"C:\\Base\\Sub".to_vec();
    let mut p = load(&initial, &[&declared, b"D:\\Data"]);
    initial.fill(b'?');
    declared.fill(b'?');
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for (input, expected) in [
        (&b".."[..], &b"C:\\Base"[..]),
        (b".\\Other\\Deep", b"C:\\Base\\Other\\Deep"),
        (b"..\\..", b"C:\\Base"),
        (b"\\BASE\\other\\", b"C:\\BASE\\other"),
        (b"d:\\data", b"d:\\data"),
        (b"\\..", b"d:\\"),
        (b"C:\\Base\\Sub", b"C:\\Base\\Sub"),
        (b"..\\\\Other\\..\\Sub\\.\\", b"C:\\Base\\Sub"),
    ] {
        change(&mut p, input, 1);
        assert_eq!(current(&mut p), expected);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    let mut other = load(b"C:\\", &[]);
    change(&mut other, b"C:\\Base", 0);
    assert_eq!(other.last_error().unwrap(), 3);
    assert_eq!(current(&mut other), b"C:\\");
}

#[test]
fn missing_and_invalid_paths_fail_without_mutating_the_current_directory() {
    let mut p = load(b"C:\\Base\\Sub", &[b"D:\\Data"]);
    for (input, error) in [
        (&b"C:\\Bas"[..], 3),
        (b"C:\\Absent", 3),
        (b"D:\\data\\child", 3),
        (b"C:\\program.exe", 3),
        (b"", 3),
        (b"a*b", 123),
        (b"ab:c", 123),
        (b"a\x01b", 123),
    ] {
        change(&mut p, input, 0);
        assert_eq!(p.last_error().unwrap(), error);
        assert_eq!(current(&mut p), b"C:\\Base\\Sub");
    }
    for input in [
        &b"C:"[..],
        b"C:relative",
        b"\\\\server\\share",
        b"\\\\?\\C:\\",
        b"C:/Base",
        b"a\xff",
        b"a\x7f",
        b"NUL",
        b"CoM1.txt",
        b"lpt9",
        b"C:\\CON",
        b"trail.",
        b"trail ",
    ] {
        p.memory.write(u64::from(SOURCE), input).unwrap();
        p.memory
            .write(u64::from(SOURCE) + input.len() as u64, &[0])
            .unwrap();
        let before = prepare(&mut p, SOURCE);
        let error = p.last_error().unwrap();
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), error);
        assert_eq!(current(&mut p), b"C:\\Base\\Sub");
    }
}

#[test]
fn namespace_count_and_payload_limits_preserve_prior_path_admission() {
    fn check(current: &[u8], directories: &[&[u8]]) -> Result<Process32, LoadError> {
        Process32::load_with_options(
            &change_directory_executable::pe32(),
            64,
            ProcessOptions {
                current_directory: current,
                directories,
                ..ProcessOptions::default()
            },
        )
    }
    let roots = vec![&b"C:\\"[..]; 256];
    assert!(check(b"C:\\", &roots[..255]).is_ok());
    assert!(matches!(
        check(b"C:\\", &roots),
        Err(LoadError::InvalidProcessParameters)
    ));
    let mut long = vec![b'a'; 32765];
    long[..3].copy_from_slice(b"C:\\");
    assert!(check(b"C:\\", &[&long, &long]).is_ok());
    let mut longer = long.clone();
    longer.push(b'a');
    assert!(matches!(
        check(b"C:\\", &[&long, &longer]),
        Err(LoadError::InvalidProcessParameters)
    ));
    for path in [
        &b"relative"[..],
        b"C:\\a\\",
        b"C:\\a\\..",
        b"C:\\a\0b",
        b"C:\\a\xff",
    ] {
        assert!(matches!(
            check(b"C:\\", &[path]),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
    let mut p = load(b"C:\\legacy. ", &[]);
    assert_eq!(current(&mut p), b"C:\\legacy. ");
    change(&mut p, b".", 1);
    assert_eq!(current(&mut p), b"C:\\legacy. ");
}

#[test]
fn failed_error_write_and_guest_reads_leave_all_state_unchanged() {
    let mut p = load(b"C:\\", &[b"C:\\Data"]);
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    change(&mut p, b"Data", 1);
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory.write(u64::from(SOURCE), b"absent\0").unwrap();
    let before = prepare(&mut p, SOURCE);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(current(&mut p), b"C:\\Data");
    assert_eq!(p.last_error().unwrap(), 77);
    for source in [0, 0x0040_2fff] {
        p.memory.write(0x0040_2fff, b"x").unwrap();
        let before = prepare(&mut p, source);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(current(&mut p), b"C:\\Data");
    }
    prepare(&mut p, SOURCE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn page_end_paths_captured_frame_and_error_cell_aliases_follow_dispatch() {
    let mut p = load(b"C:\\", &[]);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for source in [0x0040_2ffc, u32::MAX - 3] {
        p.memory.write(u64::from(source), b"C:\\\0").unwrap();
        p.memory
            .protect(u64::from(source & !0xfff), PAGE_SIZE, Permissions::READ)
            .unwrap();
        let before = prepare(&mut p, source);
        complete(&mut p, before, 1, 0x0040_1000);
        p.memory
            .protect(
                u64::from(source & !0xfff),
                PAGE_SIZE,
                Permissions::READ_WRITE,
            )
            .unwrap();
    }
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    let before = prepare(&mut p, u32::MAX);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    let before = prepare(&mut p, STACK);
    p.memory.write(u64::from(STACK), b"C:\\\0").unwrap();
    complete(&mut p, before, 1, 0x005c_3a43);
    let base = 0x1000_002a_u32;
    let source = base + 4;
    p.memory
        .write(u64::from(base), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(source), &source.to_le_bytes())
        .unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, base);
    let before = p.cpu;
    complete(&mut p, before, 1, 0x0040_1000);
    p.memory.write(0x7ffd_e034, b"C:\\\0").unwrap();
    let before = prepare(&mut p, 0x7ffd_e034);
    complete(&mut p, before, 1, 0x0040_1000);
    assert_eq!(p.last_error().unwrap(), 0x005c_3a43);
    p.memory.write(0x7ffd_e034, b"?\0").unwrap();
    let before = prepare(&mut p, 0x7ffd_e034);
    complete(&mut p, before, 0, 0x0040_1000);
    assert_eq!(p.last_error().unwrap(), 123);
    assert_eq!(current(&mut p), b"C:\\");
}

#[test]
fn maximum_path_and_unterminated_scan_are_bounded_without_partial_changes() {
    let mut long = vec![b'a'; 32767];
    long[..3].copy_from_slice(b"C:\\");
    let mut p = load(&long, &[]);
    p.memory.write(u64::from(OUTPUT), &long).unwrap();
    let before = prepare(&mut p, OUTPUT);
    complete(&mut p, before, 1, 0x0040_1000);
    assert_eq!(current(&mut p), long);
    change(&mut p, b"x\\..", 1);
    assert_eq!(current(&mut p), long);
    change(&mut p, b"b", 0);
    assert_eq!(p.last_error().unwrap(), 206);
    assert_eq!(current(&mut p), long);
    p.memory
        .write(u64::from(OUTPUT), &vec![b'x'; 32768])
        .unwrap();
    let before = prepare(&mut p, OUTPUT);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(current(&mut p), long);
}
