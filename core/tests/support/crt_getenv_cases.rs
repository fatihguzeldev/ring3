use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};

pub const NAME: u32 = 0x0040_2180;
pub const ENVIRON: u32 = 0x7000_2018;

pub fn executable(name: &[u8]) -> Vec<u8> {
    let mut code = vec![0x68];
    code.extend(NAME.to_le_bytes());
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc]);
    let mut bytes = super::imported_executable::pe32(&code, "MSVCRT.dll", &["getenv"]);
    bytes[0x580..0x580 + name.len()].copy_from_slice(name);
    bytes[0x580 + name.len()] = 0;
    bytes
}

pub fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn imported_lookup_across_budgets() {
    for (name, index, offset, value) in [
        (b"demo".as_slice(), Some(0), 5, b"a=b\xff\0".as_slice()),
        (b"eMpTy", Some(2), 6, b"\0"),
        (b"DEM", Some(3), 4, b"prefix\0"),
        (b"missing", None, 0, b""),
        (b"", None, 0, b""),
    ] {
        for budget in [1, 3, 30] {
            let mut p = Process32::load_with_options(
                &executable(name),
                32,
                ProcessOptions {
                    environment: &[
                        b"DeMo=a=b\xff",
                        b"DEMO=second",
                        b"EMPTY=",
                        b"DEM=prefix",
                        b"=C:=hidden",
                    ],
                    ..ProcessOptions::default()
                },
            )
            .unwrap();
            let expected =
                index.map_or(0, |index| word(&p, word(&p, ENVIRON) + index * 4) + offset);
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 30);
            }
            assert_eq!(counts, (4, 1));
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(p.cpu.register(Register32::Eax), expected);
            if expected != 0 {
                let mut bytes = vec![0; value.len()];
                p.memory.read(u64::from(expected), &mut bytes).unwrap();
                assert_eq!(bytes, value);
            }
        }
    }
}
