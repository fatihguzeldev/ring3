#![forbid(unsafe_code)]

use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, EXIT_OFFSET, HELPER_OFFSET, X87_OFFSET},
        memory_helper::{HELPER_SIZE, encode_helper_result},
        x86::{EXIT_SIZE, X87_SIZE, decode_x87, encode_exit_v3, encode_state, encode_x87},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{State32, X87State},
    },
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const CODE: u32 = 0x1000;

fn raw_disposal_bank() -> Vec<u8> {
    let mut bytes = vec![0xcc; 4096];
    for index in 0_u8..9 {
        let at = usize::from(index) * 0x20;
        bytes[at..at + 8].copy_from_slice(&[0x8d, 0x40, index + 1, 0xe9, 0x18, 0, 0, 0]);
    }
    bytes
}

fn authored_disposal_arena() -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..ARENA_SIZE)
        .map(|index| (index.wrapping_mul(37).wrapping_add(19)) as u8)
        .collect();
    encode_state(
        &State32 {
            registers: [
                0xffff_fff0,
                0x1111_2222,
                0x3333_4444,
                0x5555_6666,
                0x7777_8888,
                0x9999_aaaa,
                0xbbbb_cccc,
                0xdddd_eeee,
            ],
            eip: CODE,
            eflags: 0xcd7,
        },
        &mut bytes[..56],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut bytes[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    bytes[96..100].copy_from_slice(&0_u32.to_le_bytes());
    encode_helper_result(
        Ok(0xdeca_fbad),
        &mut bytes[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )
    .unwrap();
    let fp = X87State {
        control: 0x27f,
        status: 0x2840,
        tag: 0xaaaa,
        opcode: 0x345,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x9abc_def0,
        code_selector: 0x33,
        data_selector: 0x2b,
        registers: std::array::from_fn(|register| {
            std::array::from_fn(|byte| ((40 + register * 10 + byte) * 29 + 7) as u8)
        }),
    };
    encode_x87(&fp, &mut bytes[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    assert_eq!(
        decode_x87(&bytes[X87_OFFSET..X87_OFFSET + X87_SIZE]),
        Ok(fp)
    );
    bytes
}

#[test]
fn capture_installed_resident_disposal_raw_inputs_without_compilation() {
    let directory =
        std::env::var_os("RING3_INSTALLED_RESIDENT_DISPOSAL_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let bank = raw_disposal_bank();
    let initial = EngineInstance::new(1, 0x4770_0000_4449_5350)
        .unwrap()
        .arena()
        .to_vec();
    let authored = authored_disposal_arena();
    assert_eq!(
        (bank.len(), initial.len(), authored.len()),
        (4096, ARENA_SIZE, ARENA_SIZE)
    );
    for index in 0..9 {
        let at = index * 0x20;
        assert_eq!(
            &bank[at..at + 8],
            &[0x8d, 0x40, index as u8 + 1, 0xe9, 0x18, 0, 0, 0]
        );
        assert!(bank[at + 8..at + 0x20].iter().all(|&byte| byte == 0xcc));
    }
    assert_ne!(&initial[X87_OFFSET..], &authored[X87_OFFSET..]);
    if let Some(directory) = directory {
        let groups: Vec<_> = (0_u8..9)
            .map(|index| {
                format!(
                    "{{\"id\":\"{}\",\"blocks\":[[{},8]],\"instructions\":2}}",
                    char::from(b'A' + index),
                    CODE + u32::from(index) * 0x20
                )
            })
            .collect();
        let manifest = format!(
            "{{\"schema_version\":1,\"raw_inputs_only\":true,\"groups\":[{}],\"files\":[{{\"file\":\"bank.x86\",\"bytes\":4096}},{{\"file\":\"initial-arena.bin\",\"bytes\":{ARENA_SIZE}}},{{\"file\":\"authored-arena.bin\",\"bytes\":{ARENA_SIZE}}}]}}\n",
            groups.join(",")
        );
        for (file, bytes) in [
            ("bank.x86", bank),
            ("initial-arena.bin", initial),
            ("authored-arena.bin", authored),
            ("capture.json", manifest.into_bytes()),
        ] {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(file))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
}
