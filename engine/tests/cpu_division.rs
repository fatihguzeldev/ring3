use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{
            EXIT_SIZE, decode_exit, encode_exit, encode_exit_v2, encode_exit_v3, encode_exit_v4,
            encode_exit_v5,
        },
    },
    cpu::{
        ExecutionExit, ExitReason, UnsupportedFeature,
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{DivideKind, Operation},
        },
    },
    memory::GuestAddress,
    process::EngineInstance,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn admission(modrm: u8) {
    let mut engine = code(&[0xf7, modrm, 0xeb, 0]);
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&CODE.to_le_bytes());
    request[4..].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register DWORD division admits through the public compiler");
}

#[test]
fn register_div_admits_public_api() {
    admission(0xf0);
}

#[test]
fn register_idiv_admits_public_api() {
    admission(0xf8);
}

#[test]
fn version_five_accepts_dedicated_divide_error() {
    let mut bytes = [0; EXIT_SIZE];
    encode_exit(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Budget,
        },
        &mut bytes,
    )
    .unwrap();
    bytes[4..6].copy_from_slice(&5_u16.to_le_bytes());
    bytes[16..20].copy_from_slice(&10_u32.to_le_bytes());
    assert_eq!(
        decode_exit(&bytes).unwrap(),
        ExecutionExit {
            retired: 0,
            reason: ExitReason::DivideError
        }
    );
}

#[test]
fn divide_error_wire_contract_is_versioned_and_rejects_nonzero_detail() {
    let exit = ExecutionExit {
        retired: 7,
        reason: ExitReason::DivideError,
    };
    let mut bytes = [0xa5; EXIT_SIZE];
    encode_exit_v5(&exit, &mut bytes).unwrap();
    assert_eq!(&bytes[..16], b"R3EX\x05\0\x01\0\x28\0\0\0\0\0\0\0");
    assert_eq!(
        &bytes[16..],
        &[
            10, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(decode_exit(&bytes).unwrap(), exit);
    for version in 1_u16..=4 {
        let mut old = bytes;
        old[4..6].copy_from_slice(&version.to_le_bytes());
        assert!(decode_exit(&old).is_err());
    }
    for encoder in [encode_exit, encode_exit_v2, encode_exit_v3, encode_exit_v4] {
        let mut old = [0xa5; EXIT_SIZE];
        assert!(encoder(&exit, &mut old).is_err());
        assert_eq!(old, [0xa5; EXIT_SIZE]);
    }
    for offset in [24, 28, 32, 36] {
        let mut malformed = bytes;
        malformed[offset] = 1;
        assert!(decode_exit(&malformed).is_err());
    }
    for reason in [
        ExitReason::Budget,
        ExitReason::Cancelled,
        ExitReason::NeedCode,
        ExitReason::Gate { id: 7 },
        ExitReason::ProcessExited { code: 42 },
    ] {
        let exit = ExecutionExit { retired: 9, reason };
        encode_exit_v5(&exit, &mut bytes).unwrap();
        assert_eq!(decode_exit(&bytes).unwrap(), exit);
    }
}

#[test]
fn all_register_division_forms_compile_in_six_profiles_and_exclusions_stay_strict() {
    let registers = [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ];
    let mut bytes = Vec::new();
    for base in [0xf0, 0xf8] {
        for source in 0..8 {
            bytes.extend_from_slice(&[0xf7, base | source]);
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    let mut engine = code(&bytes);
    let memory = engine.memory().unwrap();
    for (group, kind) in [DivideKind::Unsigned, DivideKind::Signed]
        .into_iter()
        .enumerate()
    {
        for (index, source) in registers.into_iter().enumerate() {
            let pc = CODE + (group * 16 + index * 2) as u32;
            let instruction = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                instruction.operation(),
                &Operation::DivideAccumulator { kind, source }
            );
            assert_eq!(
                (instruction.length(), instruction.next_pc()),
                (2, GuestAddress(pc + 2))
            );
        }
    }
    let limits = CompileLimits::default();
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 34
            }],
            limits
        )
        .unwrap()
        .metadata()
        .instructions,
        17
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], limits)
            .unwrap()
            .metadata()
            .instructions,
        17
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&bytes);
            let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            request[..4].copy_from_slice(&CODE.to_le_bytes());
            request[4..].copy_from_slice(&if entries { 0_u32 } else { 34 }.to_le_bytes());
            let arena = engine.arena().to_vec();
            match (resident, entries) {
                (false, false) => {
                    engine.compile(1).unwrap();
                }
                (false, true) => {
                    engine.compile_entries(1, 0).unwrap();
                }
                (true, false) => {
                    engine.compile_resident(1).unwrap();
                }
                (true, true) => {
                    engine.compile_resident_entries(1, 0).unwrap();
                }
            }
            assert_eq!(engine.arena(), arena);
        }
    }
    for bytes in [
        &[0x66, 0xf7, 0xf0][..],
        &[0x66, 0xf7, 0xf8],
        &[0x67, 0xf7, 0xf0],
        &[0xf2, 0xf7, 0xf0],
        &[0xf3, 0xf7, 0xf8],
        &[0xf6, 0xf0],
        &[0xf6, 0xf8],
        &[0xf7, 0x30],
        &[0xf7, 0x38],
        &[0xf7, 0x34, 0x8b],
        &[0xf7, 0x3c, 0x8b],
    ] {
        engine.protect(CODE, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(CODE, bytes.len() as u32).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
}
