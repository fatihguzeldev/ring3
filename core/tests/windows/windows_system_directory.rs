use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00f8;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2280;
const PATH: &[u8; 20] = b"C:\\Windows\\System32\0";

fn load() -> Process32 {
    Process32::load_with_options(
        &imported_executable::pe32(
            &[0xcc],
            "KERNEL32.dll",
            &["GetSystemDirectoryA", "GetModuleFileNameA"],
        ),
        32,
        ProcessOptions {
            image_path: b"D:\\Games\\demo.exe",
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32, arguments: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000_u32)
        .chain(arguments)
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32, target: u32, cleanup: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + cleanup);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

#[test]
fn required_size_and_full_copy_share_the_builtin_module_directory() {
    let mut p = load();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for capacity in [0, 1, 19, 20, 21, u32::MAX] {
        p.memory.write(u64::from(OUTPUT), &[0x55; 24]).unwrap();
        let before = prepare(&mut p, API, &[OUTPUT, capacity]);
        success(
            &mut p,
            before,
            if capacity < 20 { 20 } else { 19 },
            0x0040_1000,
            12,
        );
        let mut bytes = [0; 24];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        let mut expected = [0x55; 24];
        if capacity >= 20 {
            expected[..20].copy_from_slice(PATH);
        }
        assert_eq!(bytes, expected);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    let before = prepare(&mut p, 0x7000_00e0, &[0x7000_0800, OUTPUT, 64]);
    success(&mut p, before, 32, 0x0040_1000, 16);
    let mut path = [0; 33];
    p.memory.read(u64::from(OUTPUT), &mut path).unwrap();
    assert_eq!(&path, b"C:\\Windows\\System32\\kernel32.dll\0");
    assert_eq!(&path[..19], &PATH[..19]);
}

#[test]
fn short_buffers_need_no_access_and_sufficient_buffers_check_only_twenty_bytes() {
    let mut p = load();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for pointer in [0, u32::MAX, 0x7000_0000] {
        for capacity in [0, 1, 19] {
            let before = prepare(&mut p, API, &[pointer, capacity]);
            success(&mut p, before, 20, 0x0040_1000, 12);
        }
    }
    let before = prepare(&mut p, API, &[0x0040_2fec, u32::MAX]);
    success(&mut p, before, 19, 0x0040_1000, 12);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut p, API, &[u32::MAX - 19, 20]);
    success(&mut p, before, 19, 0x0040_1000, 12);
    let mut bytes = [0; 20];
    p.memory.read(u64::from(u32::MAX - 19), &mut bytes).unwrap();
    assert_eq!(&bytes, PATH);
}

#[test]
fn invalid_output_or_frame_is_atomic_and_valid_aliases_use_captured_arguments() {
    let mut p = load();
    p.memory.write(0x0040_2ff0, &[0x55; 16]).unwrap();
    for pointer in [0, u32::MAX - 18, 0x0040_2ff0, 0x0040_1000] {
        let before = prepare(&mut p, API, &[pointer, 20]);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        let mut tail = [0; 16];
        p.memory.read(0x0040_2ff0, &mut tail).unwrap();
        assert_eq!(tail, [0x55; 16]);
    }
    let before = prepare(&mut p, API, &[STACK, 20]);
    success(&mut p, before, 19, u32::from_le_bytes(*b"C:\\W"), 12);
    let before = prepare(&mut p, API, &[0x7ffd_e034, 20]);
    success(&mut p, before, 19, 0x0040_1000, 12);
    assert_eq!(p.last_error().unwrap(), u32::from_le_bytes(*b"C:\\W"));
    prepare(&mut p, API, &[OUTPUT, 0]);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}
