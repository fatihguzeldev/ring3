use super::imported_executable;
use ring3_core::execution::{
    FileContents, FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

pub const PAYLOAD: &[u8] = b"ab\0\r\n\x1az\xffEND";

pub fn process(page_limit: u32) -> Process32 {
    Process32::load_with_options(
        &executable(),
        page_limit,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\sample.bin",
                size: PAYLOAD.len() as u64,
            }],
            file_contents: &[FileContents {
                path: b"c:\\SAMPLE.bin",
                bytes: PAYLOAD,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

pub fn verify() {
    let mut expected = None;
    for budget in [1, 100] {
        let mut p = process(128);
        let pages = p.memory.mapped_pages();
        p.memory.write(0x0040_2400, &[0x55; 16]).unwrap();
        p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
        p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
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
        assert_eq!(counts, (28, 5));
        assert_eq!(p.cpu.register(Register32::Esi), 5);
        assert_eq!(p.cpu.register(Register32::Edi), 3);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(p.last_error().unwrap(), 77);
        let mut errno = [0; 4];
        p.memory.read(0x7000_2020, &mut errno).unwrap();
        assert_eq!(u32::from_le_bytes(errno), 123);
        let mut output = [0; 16];
        p.memory.read(0x0040_2400, &mut output).unwrap();
        assert_eq!(&output[..PAYLOAD.len()], PAYLOAD);
        assert_eq!(&output[PAYLOAD.len()..14], b"END");
        assert_eq!(&output[14..], &[0x55; 2]);
        assert!(
            p.memory
                .read(u64::from(p.cpu.register(Register32::Ebx)), &mut [0])
                .is_err()
        );
        if let Some(prior) = expected {
            assert_eq!((p.cpu, counts), prior);
        }
        expected = Some((p.cpu, counts));
    }
}

pub fn executable() -> Vec<u8> {
    let mut code = Vec::new();
    for address in [0x0040_2190_u32, 0x0040_2180] {
        code.push(0x68);
        code.extend_from_slice(&address.to_le_bytes());
    }
    code.extend_from_slice(&[
        0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 8, 0x89, 0xc3, 0x50, 0x6a, 6, 0x6a, 2, 0x68,
        0, 0x24, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x83, 0xc4, 16, 0x89, 0xc6, 0x6a, 2,
        0x6a, 0xfd, 0x53, 0xff, 0x15, 0x6c, 0x20, 0x40, 0, 0x83, 0xc4, 12, 0x53, 0x6a, 3, 0x6a, 1,
        0x68, 0x0b, 0x24, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x83, 0xc4, 16, 0x89, 0xc7,
        0x53, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc,
    ]);
    let mut bytes =
        imported_executable::pe32(&code, "MSVCRT.dll", &["fopen", "fread", "fclose", "fseek"]);
    bytes[0x580..0x58b].copy_from_slice(b"sample.bin\0");
    bytes[0x590..0x593].copy_from_slice(b"rb\0");
    bytes
}
