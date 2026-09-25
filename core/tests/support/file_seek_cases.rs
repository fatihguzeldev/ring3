use ring3_core::execution::{
    Cpu32, FileContents, FileMetadata, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

pub const OPEN: u32 = 0x7000_05b4;
pub const SEEK: u32 = 0x7000_05c0;
pub const CLOSE: u32 = 0x7000_021c;
pub const FIRST: u32 = 0x7a00_0004;
pub const PATH: u32 = 0x0040_2200;
pub const STACK: u32 = 0x1000_ff00;
pub const ERROR: u32 = 0x7ffd_e034;
pub const ERRNO: u32 = 0x7000_2020;

pub fn process(flags: u32, bytes: &[u8]) -> Process32 {
    let mut code = Vec::new();
    for value in [0_u32, flags, 3, 0, 1, 0x8000_0000, PATH] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6]);
    for (method, distance, target) in [
        (0_u32, 512_u32, 0xc7_u8),
        (1, (-512_i32).cast_unsigned(), 0xc3),
    ] {
        for value in [method, 0, distance] {
            code.push(0x68);
            code.extend(value.to_le_bytes());
        }
        code.extend([0x56, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x89, target]);
    }
    code.extend([0x56, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc]);
    let exe = super::imported_executable::pe32(
        &code,
        "kErNeL32.dll",
        &["CreateFileA", "SetFilePointer", "CloseHandle"],
    );
    let mut p = Process32::load_with_options(
        &exe,
        64,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\sample.bin",
                size: bytes.len() as u64,
            }],
            file_contents: &[FileContents {
                path: b"C:\\sample.bin",
                bytes,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory
        .write(u64::from(PATH), b"C:\\sample.bin\0")
        .unwrap();
    put(&mut p, ERROR, 77);
    put(&mut p, ERRNO, 88);
    p
}

pub fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

pub fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        put(p, STACK + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu
}

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let idle = p.run(0);
    assert_eq!((idle.instructions, idle.api_calls), (0, 0));
    assert_eq!(p.cpu, expected);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let result = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    let cleanup = if matches!(
        api,
        0x7000_0188 | 0x7000_0194 | 0x7000_019c | 0x7000_01a0 | 0x7000_01a4
    ) {
        4
    } else {
        u32::try_from(args.len() + 1).unwrap() * 4
    };
    expected.set_register(Register32::Esp, STACK + cleanup);
    expected.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, expected);
    result
}

pub fn open(p: &mut Process32, flags: u32) -> u32 {
    call(p, OPEN, &[PATH, 0x8000_0000, 1, 0, 3, flags, 0])
}

pub fn seek(p: &mut Process32, handle: u32, distance: i32, origin: u32) -> u32 {
    call(p, SEEK, &[handle, distance.cast_unsigned(), 0, origin])
}

pub fn imported_seek_has_independent_position_and_preserves_size() {
    for flags in [0, 0x80, 0x2000_0000, 0x2000_0080] {
        for budget in [1, 5, 50] {
            let mut p = process(flags, &vec![0x91; 1024]);
            for (i, api) in [OPEN, SEEK, CLOSE].into_iter().enumerate() {
                assert_eq!(word(&p, 0x0040_2060 + u32::try_from(i).unwrap() * 4), api);
            }
            let pages = p.memory.mapped_pages();
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 50);
            }
            assert_eq!(counts, (24, 4));
            assert_eq!(p.cpu.register(Register32::Esi), FIRST);
            assert_eq!(p.cpu.register(Register32::Edi), 512);
            assert_eq!(p.cpu.register(Register32::Ebx), 0);
            assert_eq!(p.cpu.register(Register32::Eax), 1);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(word(&p, ERROR), 77);
            assert_eq!(word(&p, ERRNO), 88);
            assert_eq!(open(&mut p, flags), FIRST + 4);
            assert_eq!(seek(&mut p, FIRST + 4, 0, 1), 0);
            assert_eq!(seek(&mut p, FIRST + 4, 0, 2), 1024);
            assert_eq!(seek(&mut p, FIRST + 4, 512, 1), 1536);
            assert_eq!(call(&mut p, 0x7000_05b8, &[FIRST + 4, 0]), 1024);
            assert_eq!(seek(&mut p, FIRST + 4, 0, 1), 1536);
            assert_eq!(call(&mut p, CLOSE, &[FIRST + 4]), 1);
            assert_eq!(p.memory.mapped_pages(), pages);
        }
    }
}
