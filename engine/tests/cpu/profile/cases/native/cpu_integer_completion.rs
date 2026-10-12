use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32, SegmentRegister,
        atomic::AtomicKind,
        decode::decode_one,
        ir::{AddressSize, AtomicSource, ByteRegister, CountBranchKind, Operation},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> Operation {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    *decode_one(&memory, GuestAddress(0x1000))
        .unwrap_or_else(|error| panic!("decode {bytes:02x?}: {error:?}"))
        .operation()
}

#[test]
fn locked_integer_families_keep_width_source_and_effective_address_typed() {
    for (width, prefix) in [(1, vec![]), (2, vec![0x66]), (4, vec![])] {
        for (kind, opcode) in [
            (AtomicKind::Add, 0x00),
            (AtomicKind::AddCarry, 0x10),
            (AtomicKind::Subtract, 0x28),
            (AtomicKind::SubtractBorrow, 0x18),
        ] {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&[0xf0, opcode + u8::from(width != 1), 0x01]);
            match decode(&bytes) {
                Operation::AtomicMemory {
                    kind: actual,
                    width: actual_width,
                    address,
                    source,
                } => {
                    assert_eq!((actual, actual_width), (kind, width));
                    assert_eq!(address.base, Some(Register32::Ecx));
                    assert_eq!(
                        source,
                        if width == 1 {
                            AtomicSource::ByteRegister(ByteRegister::Al)
                        } else {
                            AtomicSource::Register(Register32::Eax)
                        }
                    );
                }
                operation => panic!("unexpected {bytes:02x?}: {operation:?}"),
            }
        }
        for (kind, group, opcode) in [
            (AtomicKind::Increment, 0, 0xfe),
            (AtomicKind::Decrement, 1, 0xfe),
            (AtomicKind::Negate, 3, 0xf6),
            (AtomicKind::Not, 2, 0xf6),
        ] {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&[0xf0, opcode + u8::from(width != 1), 1 | group << 3]);
            assert!(matches!(decode(&bytes), Operation::AtomicMemory {
                kind: actual, width: actual_width, source: AtomicSource::Immediate(0), ..
            } if actual == kind && actual_width == width));
        }
    }
    for (kind, opcode) in [
        (AtomicKind::And, 0x20),
        (AtomicKind::Or, 0x08),
        (AtomicKind::Xor, 0x30),
    ] {
        for (width, bytes) in [
            (1, vec![0xf0, opcode, 1]),
            (2, vec![0x66, 0xf0, opcode + 1, 1]),
        ] {
            assert!(matches!(decode(&bytes), Operation::AtomicMemory {
                kind: actual, width: actual_width, ..
            } if actual == kind && actual_width == width));
        }
    }
    for (kind, opcode) in [
        (AtomicKind::BitSet, 0xab),
        (AtomicKind::BitReset, 0xb3),
        (AtomicKind::BitComplement, 0xbb),
    ] {
        assert!(
            matches!(decode(&[0x66, 0xf0, 0x0f, opcode, 1]), Operation::AtomicMemory {
            kind: actual, width: 2, source: AtomicSource::Register(Register32::Eax), ..
        } if actual == kind)
        );
    }
    for (kind, group) in [
        (AtomicKind::BitSet, 5),
        (AtomicKind::BitReset, 6),
        (AtomicKind::BitComplement, 7),
    ] {
        for prefix in [vec![], vec![0x66]] {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&[0xf0, 0x0f, 0xba, 1 | group << 3, 0xff]);
            assert!(matches!(decode(&bytes), Operation::AtomicMemory {
                kind: actual, width, source: AtomicSource::Immediate(255), ..
            } if actual == kind && width == if prefix.is_empty() { 4 } else { 2 }));
        }
    }
    for bytes in [vec![0x0f, 0xc7, 0x09], vec![0xf0, 0x0f, 0xc7, 0x09]] {
        assert!(
            matches!(decode(&bytes), Operation::CompareExchange8B { address }
            if address.base == Some(Register32::Ecx))
        );
    }
}

#[test]
fn system_count_and_segmented_xlat_forms_have_distinct_typed_operations() {
    assert_eq!(decode(&[0x0f, 0xa2]), Operation::CpuId);
    assert_eq!(decode(&[0x0f, 0x31]), Operation::VirtualTimestamp);
    for (opcode, kind) in [
        (0xe2, CountBranchKind::Loop),
        (0xe1, CountBranchKind::LoopEqual),
        (0xe0, CountBranchKind::LoopNotEqual),
        (0xe3, CountBranchKind::EcxZero),
    ] {
        assert_eq!(
            decode(&[0x67, opcode, 5]),
            Operation::CountBranchWord {
                kind,
                target: GuestAddress(0x1008),
            }
        );
    }
    for (prefix, segment) in [(0x64, SegmentRegister::Fs), (0x65, SegmentRegister::Gs)] {
        assert!(
            matches!(decode(&[prefix, 0x67, 0xd7]), Operation::TranslateByteAddress { address }
            if address.address_size == AddressSize::Bits16 && address.segment == Some(segment))
        );
    }
}

#[test]
fn new_integer_capture_is_bound_and_does_not_access_unmapped_operands() {
    let code = [
        0xf0, 0x11, 0x01, 0x66, 0xf0, 0x0f, 0xbb, 0x01, 0x0f, 0xc7, 0x09, 0x0f, 0xa2, 0x0f, 0x31,
        0xeb, 0xfe,
    ];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, 0x494e5447).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
                .copy_from_slice(&code);
            engine.upload(0x1000, code.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            let before = engine.arena().to_vec();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
                .copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                    .copy_from_slice(&(code.len() as u32).to_le_bytes());
            }
            let state_before = engine.arena()[..140].to_vec();
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
            assert_eq!(&engine.arena()[..140], state_before);
            assert_eq!(&engine.arena()[4236..], &before[4236..]);
        }
    }
}
