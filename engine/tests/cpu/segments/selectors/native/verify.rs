use ring3_engine::{
    cpu::x86::{
        Register32,
        decode::{DecodeError, decode_one},
        ir::{Operation, Value32},
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
fn verification_register_sources_use_the_word_selector_in_every_parent_register() {
    for (write, field) in [(false, 0x20), (true, 0x28)] {
        for prefix in [&[][..], &[0x66], &[0x67], &[0x2e]] {
            for (index, source) in [
                Register32::Eax,
                Register32::Ecx,
                Register32::Edx,
                Register32::Ebx,
                Register32::Esp,
                Register32::Ebp,
                Register32::Esi,
                Register32::Edi,
            ]
            .into_iter()
            .enumerate()
            {
                let mut bytes = prefix.to_vec();
                bytes.extend([0x0f, 0x00, 0xc0 | field | index as u8]);
                assert_eq!(
                    decode(&bytes),
                    Ok(Operation::VerifySegment {
                        source: Value32::Register(source),
                        write
                    })
                );
            }
        }
    }
}

#[test]
fn verification_memory_forms_preserve_address_and_segment_semantics() {
    for field in [0x20, 0x28] {
        for bytes in [
            vec![0x0f, 0x00, field | 7],
            vec![0x66, 0x0f, 0x00, field | 7],
            vec![0x0f, 0x00, 0x40 | field | 5, 0],
            vec![0x64, 0x0f, 0x00, field | 7],
            vec![0x67, 0x0f, 0x00, field],
            vec![0x65, 0x67, 0x0f, 0x00, field | 6, 0x10, 0x50],
        ] {
            assert!(matches!(
                decode(&bytes),
                Ok(Operation::VerifySegment {
                    source: Value32::Memory(_),
                    ..
                })
            ));
        }
    }
}

#[test]
fn verification_lock_is_malformed_while_repeat_duplicate_and_unrepresented_queries_stay_refused() {
    for field in [0x20, 0x28] {
        for modrm in [field, 0xc0 | field] {
            assert_eq!(
                decode(&[0xf0, 0x0f, 0x00, modrm]),
                Err(DecodeError::InvalidEncoding)
            );
            for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
                let mut bytes = prefix.to_vec();
                bytes.extend([0x0f, 0x00, modrm]);
                assert!(matches!(decode(&bytes), Err(DecodeError::Unsupported(_))));
            }
        }
    }
    for opcode in [2, 3] {
        assert!(matches!(
            decode(&[0x0f, opcode, 0xc1]),
            Err(DecodeError::Unsupported(_))
        ));
    }
}

#[test]
fn verification_capture_does_not_eagerly_fetch_the_selector_memory_operand() {
    for field in [0x20, 0x28] {
        for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut engine = EngineInstance::new(8, 0x565252).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            let code = [0x0f, 0x00, field | 7, 0xeb, 0xfe];
            engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
            engine.upload(0x1000, code.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[144..148]
                    .copy_from_slice(&(code.len() as u32).to_le_bytes());
            }
            let state = engine.arena()[..140].to_vec();
            let result = match (resident, entries) {
                (false, false) => engine.compile(1).map(|_| ()),
                (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                (true, false) => engine.compile_resident(1).map(|_| ()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
            };
            assert_eq!(result, Ok(()), "resident={resident} entries={entries}");
            assert_eq!(&engine.arena()[..140], state.as_slice());
        }
    }
}
