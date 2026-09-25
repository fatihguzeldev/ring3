use ring3_core::execution::{
    Cpu32, FileContents, FileMetadata, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

pub const API: u32 = 0x7000_05bc;
pub const ROOT: u32 = 0x0040_2200;
pub const OUT: u32 = 0x0040_2301;
pub const STACK: u32 = 0x1000_ff00;
pub const ERROR: u32 = 0x7ffd_e034;
pub const ERRNO: u32 = 0x7000_2020;

pub fn process(
    current: &[u8],
    files: &[FileMetadata<'_>],
    contents: &[FileContents<'_>],
) -> Process32 {
    let mut code = Vec::new();
    for value in arguments().into_iter().rev() {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc]);
    let exe = super::imported_executable::pe32(&code, "kErNeL32.dll", &["GetDiskFreeSpaceA"]);
    let mut p = Process32::load_with_options(
        &exe,
        64,
        ProcessOptions {
            current_directory: current,
            directories: &[b"E:\\only"],
            files,
            file_contents: contents,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(u64::from(ROOT), b"C:\\\0").unwrap();
    for address in [OUT, OUT + 4, OUT + 8, OUT + 12] {
        put(&mut p, address, 0xfeed_abba);
    }
    put(&mut p, ERROR, 77);
    put(&mut p, ERRNO, 88);
    p
}

pub fn fixture() -> Process32 {
    process(
        b"C:\\root",
        &[
            FileMetadata {
                path: b"C:\\root\\empty",
                size: 0,
            },
            FileMetadata {
                path: b"C:\\root\\one",
                size: 1,
            },
            FileMetadata {
                path: b"C:\\root\\cluster",
                size: 4096,
            },
            FileMetadata {
                path: b"C:\\root\\spill",
                size: 4097,
            },
            FileMetadata {
                path: b"D:\\root\\other",
                size: 8193,
            },
        ],
        &[FileContents {
            path: b"C:\\root\\one",
            bytes: b"x",
        }],
    )
}

pub fn arguments() -> [u32; 5] {
    [ROOT, OUT, OUT + 4, OUT + 8, OUT + 12]
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

pub fn values(p: &Process32) -> [u32; 4] {
    [OUT, OUT + 4, OUT + 8, OUT + 12].map(|address| word(p, address))
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
    let cleanup = if api == 0x7000_0188 {
        4
    } else {
        u32::try_from(args.len() + 1).unwrap() * 4
    };
    expected.set_register(Register32::Esp, STACK + cleanup);
    expected.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, expected);
    result
}

pub fn imported_geometry_is_deterministic_and_drive_local() {
    for budget in [1, 3, 30] {
        let mut p = fixture();
        assert_eq!(word(&p, 0x0040_2060), API);
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
            assert!(counts.0 + counts.1 < 20);
        }
        assert_eq!(counts, (7, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(values(&p), [8, 512, 0, 5]);
        for (root, total) in [(b"c:\\\0", 5), (b"D:\\\0", 4), (b"e:\\\0", 1)] {
            p.memory.write(u64::from(ROOT), root).unwrap();
            assert_eq!(call(&mut p, API, &arguments()), 1);
            assert_eq!(values(&p), [8, 512, 0, total]);
        }
        let mut args = arguments();
        args[0] = 0;
        assert_eq!(call(&mut p, API, &args), 1);
        assert_eq!(values(&p), [8, 512, 0, 5]);
        assert_eq!(word(&p, ERROR), 77);
        assert_eq!(word(&p, ERRNO), 88);
        assert_eq!(p.memory.mapped_pages(), pages);
    }
}
