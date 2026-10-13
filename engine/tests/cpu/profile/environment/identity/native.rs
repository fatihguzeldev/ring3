use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{SEGMENTS_SIZE, decode_segment_context, decode_state},
    },
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::EngineInstance,
};

const PC: u32 = 0x1000;

fn memory(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = GuestAddress(pc & !0xfff);
    memory
        .map_zeroed(PageRange::new(page, 1).unwrap(), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
        .protect(PageRange::new(page, 1).unwrap(), Permissions::EXECUTE)
        .unwrap();
    memory
}

fn operation(bytes: &[u8], pc: u32) -> Result<Operation, DecodeError> {
    let decoded = decode_one(&memory(bytes, pc), GuestAddress(pc))?;
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(decoded.pc(), GuestAddress(pc));
    assert_eq!(decoded.next_pc(), GuestAddress(pc + bytes.len() as u32));
    Ok(*decoded.operation())
}

fn form(register: u8, prefix: &[u8]) -> Vec<u8> {
    [prefix, &[0xf3, 0x0f, 0xc7, 0xf8 | register]].concat()
}

fn capture(bytes: &[u8], resident: bool, entries: bool) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0x4944454e).unwrap();
    engine.map(PC, 1, 7).unwrap();
    let program = [bytes, &[0xeb, 0xfe]].concat();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + program.len()]
        .copy_from_slice(&program);
    engine.upload(PC, program.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&PC.to_le_bytes());
    if !entries {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&(program.len() as u32).to_le_bytes());
    }
    let before = engine.arena().to_vec();
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
    assert_eq!(
        &engine.arena()[..TRANSFER_OFFSET],
        &before[..TRANSFER_OFFSET]
    );
    assert_eq!(&engine.arena()[4236..], &before[4236..]);
    engine
}

fn word(engine: &EngineInstance, index: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .try_into()
            .unwrap(),
    )
}

#[test]
fn timestamp_control_keeps_aux_zero_and_one_event_per_sample() {
    assert_eq!(
        operation(&[0x0f, 0x31], PC),
        Ok(Operation::VirtualTimestamp)
    );
    assert_eq!(
        operation(&[0x0f, 0x01, 0xf9], PC),
        Ok(Operation::VirtualTimestampAux)
    );
    let mut engine = capture(&[0x0f, 0x01, 0xf9], false, false);
    for (sample, descriptor) in [0x20000, 0x20001, 0x20000].into_iter().enumerate() {
        engine.cpu_integer(descriptor, 0).unwrap();
        assert_eq!((word(&engine, 2), word(&engine, 3)), (sample as u32 + 1, 0));
        if descriptor == 0x20001 {
            assert_eq!(word(&engine, 5), 0);
        }
    }
}

#[test]
fn mandatory_f3_registers_and_ignored_prefixes_are_canonical() {
    let mut operations = Vec::new();
    for register in 0..8 {
        let canonical = operation(&form(register, &[]), PC)
            .expect("RDPID r32 is a register-only identity read");
        assert!(!matches!(
            canonical,
            Operation::VirtualTimestamp
                | Operation::VirtualTimestampAux
                | Operation::CpuId
                | Operation::Nop
        ));
        assert!(
            !operations.contains(&canonical),
            "destination register must be encoded in the operation"
        );
        operations.push(canonical);
        for prefix in [
            &[][..],
            &[0x66],
            &[0x67],
            &[0x26],
            &[0x2e],
            &[0x36],
            &[0x3e],
            &[0x64],
            &[0x65],
            &[0x66, 0x67, 0x65],
        ] {
            assert_eq!(
                operation(&form(register, prefix), PC),
                Ok(canonical),
                "r{register}/{prefix:02x?}"
            );
        }
        assert_eq!(
            operation(
                &[&[0xf3][..], &[0x66, 0x0f, 0xc7, 0xf8 | register]].concat(),
                PC
            ),
            Ok(canonical),
            "66 also leaves a full32-bit destination after mandatoryF3"
        );
    }
}

#[test]
fn identity_capture_has_no_operand_access_or_timestamp_event() {
    for resident in [false, true] {
        for entries in [false, true] {
            for register in 0..8 {
                let mut engine = capture(&form(register, &[0x66, 0x65]), resident, entries);
                // capture cannot read data or consume a virtual timestamp event.
                // only the execute-only code page exists.
                if !resident {
                    engine.cpu_integer(0x20001, 0).unwrap();
                    assert_eq!(
                        (word(&engine, 2), word(&engine, 3), word(&engine, 5)),
                        (1, 0, 0)
                    );
                }
            }
        }
    }
}

#[test]
fn complete_identity_at_execute_page_end_needs_no_extra_fetch() {
    for register in 0..8 {
        for prefix in [&[][..], &[0x66], &[0x67], &[0x65]] {
            let bytes = form(register, prefix);
            assert!(operation(&bytes, 0x2000 - bytes.len() as u32).is_ok());
        }
    }
}

#[test]
fn truncated_identity_keeps_the_exact_execute_fetch_fault() {
    for bytes in [
        &[0xf3][..],
        &[0xf3, 0x0f],
        &[0xf3, 0x0f, 0xc7],
        &[0x66, 0xf3, 0x0f, 0xc7],
    ] {
        let pc = 0x2000 - bytes.len() as u32;
        assert_eq!(
            operation(bytes, pc),
            Err(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: bytes.len() as u32 + 1,
            })
        );
    }
}

#[test]
fn lock_memory_and_random_neighbors_keep_precise_refusals() {
    for register in 0..8 {
        assert_eq!(
            operation(&form(register, &[0xf0]), PC),
            Err(DecodeError::InvalidEncoding)
        );
        for bytes in [[0x0f, 0xc7, 0xf8 | register], [0x0f, 0xc7, 0xf0 | register]] {
            assert_eq!(
                operation(&bytes, PC),
                Err(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                "RDSEED/RDRAND remain distinct: {bytes:02x?}"
            );
        }
    }
    for bytes in [
        &[0xf3, 0x0f, 0xc7, 0x38][..],
        &[0x66, 0xf3, 0x0f, 0xc7, 0x38],
    ] {
        assert_eq!(operation(bytes, PC), Err(DecodeError::InvalidEncoding));
    }
    for bytes in [
        &[0xf3, 0xf3, 0x0f, 0xc7, 0xf8][..],
        &[0x66, 0x66, 0xf3, 0x0f, 0xc7, 0xf8],
    ] {
        assert_eq!(
            operation(bytes, PC),
            Err(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
}

#[test]
fn no_access_and_debug_inputs_are_valid_architectural_records() {
    let mut engine = EngineInstance::new(1, 9).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[4..6].copy_from_slice(&2_u16.to_le_bytes());
    arena[12..16].copy_from_slice(&1_u32.to_le_bytes());
    arena[52..56].copy_from_slice(&0x102_u32.to_le_bytes());
    assert_eq!(decode_state(&arena[..56]).unwrap().eflags, 0x102);
    assert_eq!(&arena[12..16], &1_u32.to_le_bytes());
    for index in [0, 3, 4, 5] {
        arena[4524 + 16 + index * 2..4524 + 18 + index * 2].copy_from_slice(&0_u16.to_le_bytes());
    }
    assert!(decode_segment_context(&arena[4524..4524 + SEGMENTS_SIZE]).is_ok());
}
