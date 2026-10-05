use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BitScanKind, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

fn admits(opcode: u8) {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let bytes = [0x0f, opcode, 0x03, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory bit scan must admit without reading data");
}

#[test]
fn memory_bsf_admits_with_unmapped_data() {
    admits(0xbc);
}

#[test]
fn memory_bsr_admits_with_unmapped_data() {
    admits(0xbd);
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const FORMS: [(u8, BitScanKind); 2] = [(0xbc, BitScanKind::Forward), (0xbd, BitScanKind::Reverse)];
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

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn address(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    }
}

fn encoding(opcode: u8, destination: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x0f, opcode];
    bytes.extend_from_slice(tail);
    bytes[2] |= destination << 3;
    bytes
}

fn forms(opcode: u8, destination: u8) -> Vec<(Vec<u8>, EffectiveAddress)> {
    let mut forms = Vec::new();
    for (index, base) in REGISTERS.into_iter().enumerate() {
        let tail = match base {
            Register32::Esp => vec![0x04, 0x24],
            Register32::Ebp => vec![0x45, 0],
            _ => vec![index as u8],
        };
        forms.push((
            encoding(opcode, destination, &tail),
            address(Some(base), None, 1, 0),
        ));
    }
    for (tail, expected) in [
        (
            &[0x40, 0x80][..],
            address(Some(Register32::Eax), None, 1, 0xffff_ff80),
        ),
        (
            &[0x82, 0x78, 0x56, 0x34, 0x12],
            address(Some(Register32::Edx), None, 1, 0x1234_5678),
        ),
        (
            &[0x44, 0x90, 0xe0],
            address(Some(Register32::Eax), Some(Register32::Edx), 4, 0xffff_ffe0),
        ),
        (
            &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12],
            address(Some(Register32::Edx), Some(Register32::Eax), 8, 0x1234_5678),
        ),
        (
            &[0x04, 0x85, 0x10, 0x50, 0, 0],
            address(None, Some(Register32::Eax), 4, DATA + 0x10),
        ),
        (
            &[0x05, 0x10, 0x50, 0, 0],
            address(None, None, 1, DATA + 0x10),
        ),
    ] {
        forms.push((encoding(opcode, destination, tail), expected));
    }
    forms
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

#[test]
fn every_kind_destination_and_address_has_exact_ir_and_bound_admission() {
    let mut decoded_forms = 0;
    let mut profiles = 0;
    for (opcode, kind) in FORMS {
        for first_destination in [0_usize, 4] {
            let mut bytes = Vec::new();
            let mut expected = Vec::new();
            for (destination, register) in REGISTERS
                .into_iter()
                .enumerate()
                .skip(first_destination)
                .take(4)
            {
                for (form, address) in forms(opcode, destination as u8) {
                    expected.push((CODE + bytes.len() as u32, form.len(), register, address));
                    bytes.extend_from_slice(&form);
                }
            }
            assert_eq!(expected.len(), 56);
            bytes.extend_from_slice(&[0xeb, 0]);
            let engine = code(CODE, &bytes);
            let memory = engine.memory().unwrap();
            for &(pc, length, destination, address) in &expected {
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::ReadBitScan {
                        kind,
                        destination,
                        address
                    }
                );
                assert_eq!(
                    (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
                    (GuestAddress(pc), length, GuestAddress(pc + length as u32))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                decoded_forms += 1;
            }
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes);
                    engine.protect(CODE, 1, 4).unwrap();
                    describe(&mut engine, CODE, bytes.len() as u32, entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, resident, entries).unwrap();
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                        for &(pc, _, _, _) in &expected {
                            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                            assert!(engine.lookup_resident(pc + 1).is_err());
                        }
                        assert_eq!(
                            engine
                                .lookup_resident(CODE + bytes.len() as u32 - 2)
                                .unwrap()
                                .get(),
                            id
                        );
                    } else {
                        engine.guard(KEY, id as u32).unwrap();
                        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
                    }
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    for data in [0, DATA] {
                        assert!(
                            engine
                                .memory()
                                .unwrap()
                                .resolve(GuestAddress(data), Access::Read)
                                .is_err()
                        );
                    }
                    profiles += 1;
                }
            }
        }
        let bytes = encoding(opcode, 0, &[0x03]);
        let engine = code(CODE, &bytes);
        let memory = engine.memory().unwrap();
        let unsupported = CompileError::Instruction {
            pc: GuestAddress(CODE),
            cause: InstructionError::BackendUnsupported,
        };
        assert_eq!(
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default()
            )
            .err(),
            Some(unsupported),
            "{kind:?}"
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(unsupported),
            "{kind:?}"
        );
    }
    assert_eq!((decoded_forms, profiles), (224, 16));
}

#[test]
fn excluded_forms_and_exact_operand_fetch_preserve_fault_and_snapshot_boundaries() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut exclusions = 0;
    for (opcode, _) in FORMS {
        for prefix in [0x66, 0x67, 0xf2, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0] {
            let bytes = [prefix, 0x0f, opcode, 0x03];
            let engine = code(CODE, &bytes);
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode_error,
            };
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(expected),
                "{bytes:02x?}"
            );
            exclusions += 1;
        }
        for tail in [
            &[0x03][..],
            &[0x05, 0x10, 0x50, 0, 0],
            &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12],
        ] {
            let bytes = encoding(opcode, 0, tail);
            for pc in [
                0x2000 - bytes.len() as u32,
                u32::MAX - (bytes.len() as u32 - 1),
                0x1fff,
            ] {
                let mut engine = code(pc, &bytes);
                engine
                    .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                    .unwrap();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (
                        bytes.len(),
                        GuestAddress(pc.wrapping_add(bytes.len() as u32))
                    )
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                describe(&mut engine, pc, bytes.len() as u32, false);
                let before = engine.arena().to_vec();
                let generation = engine.compile(1).unwrap();
                engine.guard(KEY, generation).unwrap();
                assert_eq!(engine.arena(), before);
            }
            let missing = &bytes[..bytes.len() - 1];
            for top in [false, true] {
                let pc = if top {
                    ((1_u64 << 32) - missing.len() as u64) as u32
                } else {
                    0x2000 - missing.len() as u32
                };
                let engine = code(pc, missing);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        bytes.len() as u32,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        }
                    ))
                );
            }
            let mut engine = code(0x1fff, &bytes);
            engine.protect(0x2000, 1, 3).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).err(),
                Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
            );
        }
        let bytes = encoding(opcode, 0, &[0x05, 0x10, 0x50, 0, 0]);
        for changed in [0x1fff, 0x2000] {
            let mut engine = code(0x1ffc, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffc)).unwrap();
            describe(&mut engine, 0x1ffc, bytes.len() as u32, false);
            let generation = engine.compile(1).unwrap();
            let id = engine.compile_resident(1).unwrap().get();
            upload(
                &mut engine,
                changed,
                &[if changed == 0x1fff { 0x10 } else { 0x50 }],
            );
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert!(engine.guard(KEY, generation).is_err());
            assert!(engine.guard_resident(KEY, id).is_err());
        }
    }
    for bytes in [[0xf3, 0x0f, 0xbc, 0x03], [0xf3, 0x0f, 0xbd, 0x03]] {
        let engine = code(CODE, &bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode_error),
            "{bytes:02x?}"
        );
        exclusions += 1;
    }
    assert_eq!(exclusions, 22);
}

#[test]
fn data_only_changes_keep_owners_current_and_operand_writes_invalidate() {
    let mut bytes = encoding(0xbc, 0, &[0x05, 0x10, 0x50, 0, 0]);
    bytes.extend_from_slice(&encoding(0xbd, 2, &[0x05, 0x10, 0x50, 0, 0]));
    bytes.extend_from_slice(&[0xeb, 0]);
    let mut engine = code(CODE, &bytes);
    let snapshots: Vec<_> = [CODE, CODE + 7]
        .into_iter()
        .map(|pc| decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap())
        .collect();
    describe(&mut engine, CODE, bytes.len() as u32, false);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    let before = (
        engine.artifact_bytes().unwrap().to_vec(),
        engine.resident_bytes(id).unwrap().to_vec(),
    );
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA + 0x10, 0x100).unwrap();
    engine.protect(DATA, 1, 1).unwrap();
    for phase in 0..3 {
        if phase == 1 {
            engine.unmap(DATA, 1).unwrap();
        } else if phase == 2 {
            engine.map(DATA, 1, 3).unwrap();
            engine.write32(DATA + 0x10, 0x101).unwrap();
            engine.protect(DATA, 1, 1).unwrap();
        }
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, id).unwrap();
        assert_eq!(
            (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(id).unwrap().to_vec()
            ),
            before
        );
        for decoded in &snapshots {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
    }
    engine.write32(CODE + 3, DATA + 0x10).unwrap();
    for decoded in snapshots {
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    assert!(engine.guard(KEY, generation).is_err());
    assert!(engine.guard_resident(KEY, id).is_err());
}

#[test]
fn late_memory_bit_scan_failures_preserve_both_publications() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for first in [0xbc, 0xbd] {
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0x0f, first, 0x03, 0x0f, 0x0b],
                5,
                instruction_error(CODE + 3, opcode_error),
            ),
            (
                CODE,
                vec![0x0f, first, 0x03, 0x66, 0x0f, first ^ 1, 0x03],
                7,
                instruction_error(CODE + 3, opcode_error),
            ),
            (
                0x1ffc,
                vec![0x0f, first, 0x03, 0x0f],
                5,
                instruction_error(
                    0x1fff,
                    fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
                ),
            ),
        ] {
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes);
                    engine.map(KEEP, 1, 7).unwrap();
                    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                    describe(&mut engine, KEEP, 3, false);
                    let generation = engine.compile(1).unwrap();
                    let keep = engine.compile_resident(1).unwrap().get();
                    describe(&mut engine, pc, declared, entries);
                    let before = (
                        engine.arena().to_vec(),
                        engine.generation(),
                        engine.artifact_bytes().unwrap().to_vec(),
                        engine.resident_bytes(keep).unwrap().to_vec(),
                    );
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(expected))
                    } else {
                        HostError::Compile(expected)
                    };
                    assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                    assert_eq!(
                        (
                            engine.arena().to_vec(),
                            engine.generation(),
                            engine.artifact_bytes().unwrap().to_vec(),
                            engine.resident_bytes(keep).unwrap().to_vec()
                        ),
                        before
                    );
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                    assert!(engine.lookup_resident(pc).is_err());
                    refusals += 1;
                }
            }
        }
    }
    assert_eq!(refusals, 24);
}
