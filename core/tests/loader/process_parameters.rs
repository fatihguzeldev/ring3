use super::imported_executable;

use ring3_core::execution::{LoadError, Process32, ProcessOptions};

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn string(process: &Process32, address: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for offset in 0..32768 {
        let mut byte = [0];
        process
            .memory
            .read(u64::from(address + offset), &mut byte)
            .unwrap();
        if byte[0] == 0 {
            return bytes;
        }
        bytes.push(byte[0]);
    }
    panic!("unterminated string");
}

#[test]
fn process_parameters_preserve_command_line_and_parse_crt_arguments() {
    for (line, expected) in [
        (
            b"\"C:\\Game Files\\game.exe\" \"a b c\" d e".as_slice(),
            vec![b"C:\\Game Files\\game.exe".as_slice(), b"a b c", b"d", b"e"],
        ),
        (
            br#"app "ab\"c" "\\" d"#.as_slice(),
            vec![b"app".as_slice(), b"ab\"c", b"\\", b"d"],
        ),
        (
            br#"app a\\\b d"e f"g h"#.as_slice(),
            vec![b"app".as_slice(), br"a\\\b", b"de fg", b"h"],
        ),
        (
            br#"app a\\\"b c d"#.as_slice(),
            vec![b"app".as_slice(), b"a\\\"b", b"c", b"d"],
        ),
        (
            br#"app a\\\\"b c" d e"#.as_slice(),
            vec![b"app".as_slice(), br"a\\b c", b"d", b"e"],
        ),
        (
            br#"app a"b"" c d"#.as_slice(),
            vec![b"app".as_slice(), b"ab\" c d"],
        ),
        (
            b"app\t\"\"  tail\t".as_slice(),
            vec![b"app".as_slice(), b"", b"tail"],
        ),
        (b"".as_slice(), vec![b"".as_slice()]),
    ] {
        let bytes = imported_executable::pe32(
            &[0xcc],
            "MSVCRT.dll",
            &["_acmdln", "__argc", "__argv", "_environ"],
        );
        let environment: &[&[u8]] = &[b"NAME=value", b"EMPTY="];
        let process = Process32::load_with_options(
            &bytes,
            32,
            ProcessOptions {
                command_line: line,
                environment,
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            string(&process, word(&process, word(&process, 0x0040_2060))),
            line
        );
        let argument_count = word(&process, word(&process, 0x0040_2064));
        let argv = word(&process, word(&process, 0x0040_2068));
        assert_eq!(argument_count as usize, expected.len());
        for (index, value) in expected.iter().enumerate() {
            assert_eq!(
                string(
                    &process,
                    word(&process, argv + u32::try_from(index).unwrap() * 4)
                ),
                *value
            );
        }
        assert_eq!(word(&process, argv + argument_count * 4), 0);
        let env = word(&process, word(&process, 0x0040_206c));
        assert_eq!(string(&process, word(&process, env)), b"NAME=value");
        assert_eq!(string(&process, word(&process, env + 4)), b"EMPTY=");
        assert_eq!(word(&process, env + 8), 0);
        assert!(process.memory.fetch(u64::from(argv), &mut [0]).is_err());
    }
}

#[test]
fn parameter_validation_rejects_nul_invalid_environment_and_bounded_size() {
    let bytes = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["__argc"]);
    let long = vec![b'a'; 32768];
    for line in [b"app\0hidden".as_slice(), long.as_slice()] {
        assert!(matches!(
            Process32::load_with_options(
                &bytes,
                32,
                ProcessOptions {
                    command_line: line,
                    ..ProcessOptions::default()
                }
            ),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
    let long_entry = vec![b'='; 65536];
    let too_many: Vec<&[u8]> = vec![b"A=B"; 257];
    for environment in [
        &[b"MISSING".as_slice()][..],
        &[b"A=\0B".as_slice()][..],
        &[long_entry.as_slice()][..],
        too_many.as_slice(),
    ] {
        assert!(matches!(
            Process32::load_with_options(
                &bytes,
                64,
                ProcessOptions {
                    environment,
                    ..ProcessOptions::default()
                }
            ),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
}
