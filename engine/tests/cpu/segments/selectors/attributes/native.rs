use ring3_engine::{
    cpu::x86::{
        Register32, SegmentQuery, SegmentRegister,
        decode::{DecodeError, decode_one},
        ir::{AddressSize, Operation, Value32},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
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
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    Ok(*instruction.operation())
}

#[test]
fn descriptor_queries_decode_every_register_alias_and_word_destination() {
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
    for (opcode, kind) in [(2, SegmentQuery::AccessRights), (3, SegmentQuery::Limit)] {
        for (word, prefix) in [(false, &[][..]), (true, &[0x66][..])] {
            for (destination, dst) in registers.into_iter().enumerate() {
                for (source, src) in registers.into_iter().enumerate() {
                    let mut bytes = prefix.to_vec();
                    bytes.extend([0x0f, opcode, 0xc0 | (destination as u8) << 3 | source as u8]);
                    assert_eq!(
                        decode(&bytes),
                        Ok(Operation::QuerySegment {
                            kind,
                            source: Value32::Register(src),
                            destination: dst,
                            word
                        })
                    );
                }
            }
        }
    }
}

#[test]
fn descriptor_query_memory_sources_retain_selected_segment_and_address_size() {
    for opcode in [2, 3] {
        for (prefix, modrm, tail, segment, address16) in [
            (&[][..], 7, &[][..], SegmentRegister::Ds, false),
            (&[][..], 0x45, &[0][..], SegmentRegister::Ss, false),
            (&[0x64][..], 7, &[][..], SegmentRegister::Fs, false),
            (&[0x67][..], 0, &[][..], SegmentRegister::Ds, true),
            (
                &[0x65, 0x67][..],
                6,
                &[0x10, 0x50][..],
                SegmentRegister::Gs,
                true,
            ),
        ] {
            let mut bytes = prefix.to_vec();
            bytes.extend([0x0f, opcode, modrm]);
            bytes.extend(tail);
            let Operation::QuerySegment {
                source: Value32::Memory(address),
                ..
            } = decode(&bytes).unwrap()
            else {
                panic!("memory selector expected")
            };
            assert_eq!(address.resolved_segment(), segment);
            assert_eq!(address.address_size == AddressSize::Bits16, address16);
        }
    }
}

#[test]
fn descriptor_query_lock_is_invalid_and_repeat_duplicate_profiles_remain_refused() {
    for opcode in [2, 3] {
        for modrm in [7, 0xc1] {
            assert_eq!(
                decode(&[0xf0, 0x0f, opcode, modrm]),
                Err(DecodeError::InvalidEncoding)
            );
            for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
                let mut bytes = prefix.to_vec();
                bytes.extend([0x0f, opcode, modrm]);
                assert!(matches!(decode(&bytes), Err(DecodeError::Unsupported(_))));
            }
        }
    }
}

#[test]
fn descriptor_query_capture_in_every_owner_does_not_read_selector_memory_eagerly() {
    for opcode in [2, 3] {
        for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut engine = EngineInstance::new(8, 0x4c4152).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            let code = [0x0f, opcode, 7, 0xeb, 0xfe];
            engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
            engine.upload(0x1000, 5).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[144..148].copy_from_slice(&5_u32.to_le_bytes());
            }
            let before = engine.arena()[..140].to_vec();
            match (resident, entries) {
                (false, false) => engine.compile(1).map(|_| ()),
                (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                (true, false) => engine.compile_resident(1).map(|_| ()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
            }
            .unwrap();
            assert_eq!(&engine.arena()[..140], before);
        }
    }
}

#[test]
fn descriptor_queries_stay_closed_in_all_standalone_preparation_and_compile_apis() {
    use ring3_engine::cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
        compile_region, prepare_entry_region, prepare_region,
    };
    for opcode in [2, 3] {
        for word in [false, true] {
            for modrm in [7, 0xc1] {
                let mut bytes = if word { vec![0x66] } else { vec![] };
                bytes.extend([0x0f, opcode, modrm, 0xeb, 0]);
                let mut memory = AddressSpace::new(1).unwrap();
                memory
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                memory.write(GuestAddress(0x1000), &bytes).unwrap();
                let specs = [BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: bytes.len() as u32,
                }];
                let expected = Some(CompileError::Instruction {
                    pc: GuestAddress(0x1000),
                    cause: InstructionError::BackendUnsupported,
                });
                assert_eq!(
                    prepare_region(&memory, &specs, CompileLimits::default()).err(),
                    expected
                );
                assert_eq!(
                    compile_region(&memory, &specs, CompileLimits::default()).err(),
                    expected
                );
                assert_eq!(
                    prepare_entry_region(
                        &memory,
                        &[GuestAddress(0x1000)],
                        CompileLimits::default()
                    )
                    .err(),
                    expected
                );
                assert_eq!(
                    compile_entry_region(
                        &memory,
                        &[GuestAddress(0x1000)],
                        CompileLimits::default()
                    )
                    .err(),
                    expected
                );
            }
        }
    }
}
