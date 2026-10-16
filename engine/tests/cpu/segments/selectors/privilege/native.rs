use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn assert_decodes(bytes: &[u8]) {
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
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(matches!(
        instruction.operation(),
        Operation::AdjustPrivilege { .. }
    ));
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
}

#[test]
fn arpl_decodes_every_word_register_pair_including_source_destination_aliases() {
    for destination in 0..8 {
        for source in 0..8 {
            assert_decodes(&[0x63, 0xc0 | source << 3 | destination]);
            assert_decodes(&[0x66, 0x63, 0xc0 | source << 3 | destination]);
        }
    }
}

#[test]
fn arpl_memory_forms_keep_word_access_and_address_segment_prefixes() {
    for source in 0..8 {
        assert_decodes(&[0x63, source << 3 | 7]);
        assert_decodes(&[0x66, 0x63, source << 3 | 7]);
        assert_decodes(&[0x64, 0x63, source << 3 | 4, 0x89]);
        assert_decodes(&[0x67, 0x63, source << 3]);
        assert_decodes(&[0x65, 0x67, 0x63, source << 3 | 6, 0x11, 0x80]);
    }
}

#[test]
fn arpl_ignored_prefixes_do_not_change_its_fixed_word_operands() {
    for prefix in [0x2e, 0x67] {
        assert_decodes(&[prefix, 0x63, 0xd1]);
    }
}

#[test]
fn arpl_lock_prefix_is_invalid_for_register_and_memory_destinations() {
    for modrm in [0xc1, 0x01] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1000), &[0xf0, 0x63, modrm])
            .unwrap();
        crate::support_fetch::assert_decode_contract(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding,
        );
    }
}

#[test]
fn arpl_reserved_rep_prefixes_remain_explicitly_unsupported() {
    for prefix in [0xf2, 0xf3] {
        for modrm in [0xc1, 0x01] {
            let mut memory = AddressSpace::new(1).unwrap();
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory
                .write(GuestAddress(0x1000), &[prefix, 0x63, modrm])
                .unwrap();
            assert!(matches!(
                decode_one(&memory, GuestAddress(0x1000)),
                Err(DecodeError::Unsupported(_))
            ));
        }
    }
}

#[test]
fn arpl_bound_capture_does_not_eagerly_read_or_write_the_data_operand() {
    for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut engine = EngineInstance::new(8, 0x4152504c).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        let code = [0x63, 0x07, 0xeb, 0xfe];
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
        engine.upload(0x1000, code.len() as u32).unwrap();
        engine.protect(0x1000, 1, 4).unwrap();
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000u32.to_le_bytes());
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
