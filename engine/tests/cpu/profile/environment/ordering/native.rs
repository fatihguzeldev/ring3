use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32, SegmentRegister,
        decode::{DecodeError, decode_one},
        ir::{AddressSize, EffectiveAddress, Operation},
        simd::SimdKind,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
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
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    Ok(*instruction.operation())
}

#[test]
fn clflush_memory_forms_preserve_addressing_without_loading_an_operand() {
    for (bytes, address) in [
        (
            vec![0x0f, 0xae, 0x38],
            EffectiveAddress {
                base: Some(Register32::Eax),
                ..Default::default()
            },
        ),
        (
            vec![0x0f, 0xae, 0x3b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                ..Default::default()
            },
        ),
        (
            vec![0x0f, 0xae, 0x7c, 0x8c, 0x7f],
            EffectiveAddress {
                base: Some(Register32::Esp),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0x7f,
                ..Default::default()
            },
        ),
        (
            vec![0x0f, 0xae, 0x3d, 0x78, 0x56, 0x34, 0x12],
            EffectiveAddress {
                displacement: 0x12345678,
                ..Default::default()
            },
        ),
        (
            vec![0x67, 0x0f, 0xae, 0x38],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Esi),
                address_size: AddressSize::Bits16,
                ..Default::default()
            },
        ),
        (
            vec![0x2e, 0x0f, 0xae, 0x3b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                segment: Some(SegmentRegister::Cs),
                ..Default::default()
            },
        ),
        (
            vec![0x64, 0x0f, 0xae, 0x3b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                segment: Some(SegmentRegister::Fs),
                ..Default::default()
            },
        ),
        (
            vec![0x65, 0x67, 0x0f, 0xae, 0x38],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Esi),
                segment: Some(SegmentRegister::Gs),
                address_size: AddressSize::Bits16,
                ..Default::default()
            },
        ),
    ] {
        assert_eq!(
            decode(&bytes),
            Ok(Operation::CacheFlush { address }),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn rdtscp_accepts_fixed_register_outputs_with_ignored_size_and_segment_prefixes() {
    for prefixes in [&[][..], &[0x66][..], &[0x67][..], &[0x2e][..]] {
        let mut bytes = prefixes.to_vec();
        bytes.extend([0x0f, 0x01, 0xf9]);
        assert_eq!(
            decode(&bytes),
            Ok(Operation::VirtualTimestampAux),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn cache_timestamp_lock_is_invalid_and_sfence_remains_distinct() {
    for bytes in [&[0xf0, 0x0f, 0xae, 0x3b][..], &[0xf0, 0x0f, 0x01, 0xf9][..]] {
        assert_eq!(decode(bytes), Err(DecodeError::InvalidEncoding));
    }
    assert!(matches!(
        decode(&[0x0f, 0xae, 0xf8]),
        Ok(Operation::Simd(operation)) if operation.kind == SimdKind::Fence
    ));
}

#[test]
fn cache_timestamp_capture_is_bound_without_reading_the_unmapped_data_address() {
    let code = [0x0f, 0xae, 0x3b, 0x0f, 0x01, 0xf9, 0xeb, 0];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, 0x43545343).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
                .copy_from_slice(&code);
            engine.upload(0x1000, code.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
                .copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                    .copy_from_slice(&(code.len() as u32).to_le_bytes());
            }
            let before = engine.arena()[..140].to_vec();
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
            assert_eq!(&engine.arena()[..140], before);
        }
    }
}

fn word(engine: &EngineInstance, index: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .try_into()
            .unwrap(),
    )
}

#[test]
fn rdtscp_helper_shares_virtual_counter_clears_aux_and_refuses_unknown_descriptors() {
    let mut engine = EngineInstance::new(1, 0x43545343).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0x0f, 0x31, 0xeb, 0]);
    engine.upload(0x1000, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 100).unwrap();
    engine.observe_cpu_clock(117).unwrap();
    let payload = [
        0xfeedface_u32,
        0x244cd7,
        0x12345678,
        0x87654321,
        0xabcdef01,
        0xdeadbeef,
    ];
    for (index, value) in payload.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    engine.cpu_integer(0x20000, 0).unwrap();
    assert_eq!((word(&engine, 2), word(&engine, 3)), (17, 0));
    engine.cpu_integer(0x20001, 0).unwrap();
    assert_eq!(
        (word(&engine, 2), word(&engine, 3), word(&engine, 5)),
        (17, 0, 0)
    );
    assert_eq!(
        (word(&engine, 0), word(&engine, 1), word(&engine, 4)),
        (payload[0], payload[1], payload[4])
    );
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_integer(0x20002, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    engine.advance_cpu_clock(5).unwrap();
    engine.cpu_integer(0x20000, 0).unwrap();
    assert_eq!(
        (word(&engine, 2), word(&engine, 3), word(&engine, 5)),
        (22, 0, 0)
    );
}
