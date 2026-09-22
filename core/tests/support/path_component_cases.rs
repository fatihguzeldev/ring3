use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

use super::imported_executable;

pub const SOURCE: u32 = 0x0040_2180;
pub const OUTPUTS: [u32; 4] = [0x0040_2200, 0x0040_2300, 0x0040_2400, 0x0040_2500];

pub fn executable() -> Vec<u8> {
    let mut code = vec![0xb8, 99, 0, 0, 0];
    for address in OUTPUTS.into_iter().rev().chain([SOURCE]) {
        code.push(0x68);
        code.extend_from_slice(&address.to_le_bytes());
    }
    code.extend_from_slice(&[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 20, 0xcc]);
    imported_executable::pe32(&code, "mSvCrT.dLl", &["_splitpath"])
}

pub fn verify() {
    let cases: &[(&[u8], [&[u8]; 4])] = &[
        (
            b"C:\\folder/sub.dir\\file.tar.gz",
            [b"C:", b"\\folder/sub.dir\\", b"file.tar", b".gz"],
        ),
        (b"notes.txt", [b"", b"", b"notes", b".txt"]),
        (b"q:notes", [b"q:", b"", b"notes", b""]),
        (
            b"\\\\server\\share\\file",
            [b"", b"\\\\server\\share\\", b"file", b""],
        ),
        (b"a.b/", [b"", b"a.b/", b"", b""]),
        (b".profile", [b"", b"", b"", b".profile"]),
        (b"..", [b"", b"", b".", b"."]),
        (b"a.", [b"", b"", b"a", b"."]),
        (b"", [b"", b"", b"", b""]),
        (b"1:dir\\a:b.c", [b"1:", b"dir\\", b"a:b", b".c"]),
        (b"/\x81\\\xff.\x80", [b"", b"/\x81\\", b"\xff", b".\x80"]),
    ];
    for &(source, outputs) in cases {
        let mut expected = None;
        for budget in [1, 100] {
            let mut p = Process32::load(&executable(), 32).unwrap();
            p.memory.write(u64::from(SOURCE), source).unwrap();
            p.memory
                .write(u64::from(SOURCE) + source.len() as u64, &[0])
                .unwrap();
            for output in OUTPUTS {
                p.memory.write(u64::from(output), &[0x55; 256]).unwrap();
            }
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
            assert_eq!(counts, (9, 1));
            assert_eq!(p.cpu.register(Register32::Eax), 99);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(p.last_error().unwrap(), 77);
            let mut errno = [0; 4];
            p.memory.read(0x7000_2020, &mut errno).unwrap();
            assert_eq!(u32::from_le_bytes(errno), 123);
            for (address, bytes) in OUTPUTS.into_iter().zip(outputs) {
                let mut actual = vec![0; bytes.len() + 2];
                p.memory.read(u64::from(address), &mut actual).unwrap();
                assert_eq!(&actual[..bytes.len()], bytes);
                assert_eq!(&actual[bytes.len()..], &[0, 0x55]);
            }
            if let Some(prior) = expected {
                assert_eq!((p.cpu, counts), prior);
            }
            expected = Some((p.cpu, counts));
        }
    }
}
