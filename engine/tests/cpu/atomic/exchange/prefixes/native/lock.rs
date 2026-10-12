use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        x86::{
            Register32, SegmentRegister,
            decode::{DecodeError, decode_one},
            ir::{AddressSize, ByteRegister, EffectiveAddress, Operation},
        },
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x4c4f_434b_5052_4546;
const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];
const BYTES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(PC, bytes.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine
}

fn assert_operation(bytes: &[u8], expected: Operation) {
    let engine = engine(bytes);
    let before = engine.arena().to_vec();
    let instruction = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
    assert_eq!(instruction.operation(), &expected, "{bytes:02x?}");
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(instruction.next_pc(), GuestAddress(PC + bytes.len() as u32));
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(instruction.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn lawful_lock_exchange_and_both_word_prefix_orders_keep_exact_typed_operands() {
    let address = EffectiveAddress {
        base: Some(Register32::Ecx),
        ..Default::default()
    };
    for source in 0..8 {
        let modrm = 1 | ((source as u8) << 3);
        assert_operation(
            &[0xf0, 0x86, modrm],
            Operation::MemoryExchangeByte {
                address,
                source: BYTES[source],
            },
        );
        assert_operation(
            &[0xf0, 0x87, modrm],
            Operation::MemoryExchange {
                address,
                source: REGISTERS[source],
            },
        );
        for prefix in [[0x66, 0xf0], [0xf0, 0x66]] {
            assert_operation(
                &[prefix[0], prefix[1], 0x87, modrm],
                Operation::MemoryExchangeWord {
                    address,
                    source: REGISTERS[source],
                },
            );
            assert_operation(
                &[prefix[0], prefix[1], 0x0f, 0xb1, modrm],
                Operation::LockedMemoryCompareExchangeWord {
                    address,
                    source: REGISTERS[source],
                },
            );
            assert_operation(
                &[prefix[0], prefix[1], 0x0f, 0xc1, modrm],
                Operation::LockedMemoryExchangeAddWord {
                    address,
                    source: REGISTERS[source],
                },
            );
        }
    }
    // The newly admitted forms retain source/address aliases and selected segment metadata.
    let address16 = EffectiveAddress {
        base: Some(Register32::Ebx),
        index: Some(Register32::Esi),
        segment: Some(SegmentRegister::Fs),
        address_size: AddressSize::Bits16,
        ..Default::default()
    };
    assert_operation(
        &[0x64, 0x67, 0xf0, 0x66, 0x87, 0x18],
        Operation::MemoryExchangeWord {
            address: address16,
            source: Register32::Ebx,
        },
    );
    let sib = EffectiveAddress {
        base: Some(Register32::Eax),
        index: Some(Register32::Ecx),
        scale: 4,
        displacement: 0xffff_fff0,
        segment: Some(SegmentRegister::Gs),
        ..Default::default()
    };
    assert_operation(
        &[0x65, 0xf0, 0x66, 0x0f, 0xc1, 0x44, 0x88, 0xf0],
        Operation::LockedMemoryExchangeAddWord {
            address: sib,
            source: Register32::Eax,
        },
    );
}

#[test]
fn lock_register_forms_and_duplicate_or_repeat_prefixes_remain_refused_without_publication() {
    for modrm in 0xc0..=0xff {
        for bytes in [
            vec![0xf0, 0x86, modrm],
            vec![0xf0, 0x87, modrm],
            vec![0x66, 0xf0, 0x87, modrm],
            vec![0xf0, 0x66, 0x87, modrm],
            vec![0x66, 0xf0, 0x0f, 0xb1, modrm],
            vec![0xf0, 0x66, 0x0f, 0xb1, modrm],
            vec![0x66, 0xf0, 0x0f, 0xc1, modrm],
            vec![0xf0, 0x66, 0x0f, 0xc1, modrm],
        ] {
            let engine = engine(&bytes);
            let before = engine.arena().to_vec();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(PC)).err(),
                Some(DecodeError::InvalidEncoding),
                "{bytes:02x?}"
            );
            assert_eq!(engine.arena(), before);
        }
    }
    for bytes in [
        vec![0xf0, 0xf0, 0x87, 0x01],
        vec![0xf0, 0x66, 0x66, 0x87, 0x01],
        vec![0xf2, 0xf0, 0x87, 0x01],
        vec![0xf3, 0xf0, 0x86, 0x01],
        vec![0xf0, 0x66, 0x66, 0x0f, 0xb1, 0x01],
        vec![0xf0, 0x66, 0xf2, 0x0f, 0xc1, 0x01],
    ] {
        let engine = engine(&bytes);
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(PC)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn every_bound_capture_reuses_existing_atomic_helpers_without_reading_operands() {
    let bytes = [
        0xf0, 0x86, 0x21, // XCHG [ECX], AH
        0xf0, 0x66, 0x87, 0x21, // XCHG [ECX], SP
        0xf0, 0x87, 0x21, // XCHG [ECX], ESP
        0xf0, 0x66, 0x0f, 0xb1, 0x21, 0xf0, 0x66, 0x0f, 0xc1, 0x21, 0xeb, 0xfe,
    ];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = engine(&bytes);
            let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            transfer[..4].copy_from_slice(&PC.to_le_bytes());
            transfer[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            let before = engine.arena().to_vec();
            let module = match (resident, entries) {
                (false, false) => {
                    engine.compile(1).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                }
                (false, true) => {
                    engine.compile_entries(1, 0).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                }
                (true, false) => {
                    let id = engine.compile_resident(1).unwrap();
                    engine.resident_bytes(id.get()).unwrap().to_vec()
                }
                (true, true) => {
                    let id = engine.compile_resident_entries(1, 0).unwrap();
                    engine.resident_bytes(id.get()).unwrap().to_vec()
                }
            };
            for (normal, bound) in [
                ("exchange8", "exchange_resident8"),
                ("exchange16", "exchange_resident16"),
                ("exchange32", "exchange_resident32"),
                ("compare_exchange16", "compare_exchange_resident16"),
                ("exchange_add16", "exchange_add_resident16"),
            ] {
                let name = if resident { bound } else { normal };
                assert!(
                    module
                        .windows(name.len())
                        .any(|part| part == name.as_bytes()),
                    "{name}"
                );
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x5000), Access::Read)
                    .is_err()
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(PC), Access::Read)
                    .is_err()
            );
        }
    }
}
