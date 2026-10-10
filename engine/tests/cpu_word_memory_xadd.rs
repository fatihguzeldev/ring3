use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574d_5841_4444_3031;
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

fn encoding(source: u8) -> Vec<u8> {
    vec![0x66, 0x0f, 0xc1, 0x03 | source << 3]
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine
}

#[test]
fn word_memory_xadd_all_sources_decode_without_accessing_data() {
    for source in 0..8 {
        let bytes = encoding(source);
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD memory XADD must decode without accessing data");
        assert_eq!(
            decoded.operation(),
            &Operation::MemoryExchangeAddWord {
                address: addresses()[0].1,
                source: REGISTERS[usize::from(source)],
            }
        );
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    }
}

fn bound_admission(resident: bool, entries: bool) {
    for source in 0..8 {
        let mut bytes = encoding(source);
        bytes.extend([0xeb, 0]);
        assert_bound_admission(&bytes, resident, entries);
    }
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[4..]
        .copy_from_slice(&if entries { 0_u32 } else { bytes.len() as u32 }.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
    .expect("WORD memory XADD must compile bound without accessing data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert!(module.len() <= CompileLimits::default().wasm_bytes);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_memory_xadd_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_memory_xadd_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_memory_xadd_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_memory_xadd_compiles_resident_entry() {
    bound_admission(true, true);
}

fn instruction(source: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, 0x0f, 0xc1, tail[0] | source << 3];
    bytes.extend_from_slice(&tail[1..]);
    bytes
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn addresses() -> [(&'static [u8], EffectiveAddress); 13] {
    use Register32::{Ebp, Ebx, Ecx, Esi, Esp};
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    [
        (&[0x03], address(Some(Ebx), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Ebp), None, 1, 0)),
        (&[0x43, 0x80], address(Some(Ebx), None, 1, 0xffff_ff80)),
        (&[0x43, 0x7f], address(Some(Ebx), None, 1, 127)),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            address(Some(Ebx), None, 1, 0x9234_5678),
        ),
        (&[0x04, 0x0b], address(Some(Ebx), Some(Ecx), 1, 0)),
        (&[0x04, 0x4b], address(Some(Ebx), Some(Ecx), 2, 0)),
        (&[0x04, 0x8b], address(Some(Ebx), Some(Ecx), 4, 0)),
        (&[0x04, 0xcb], address(Some(Ebx), Some(Ecx), 8, 0)),
        (
            &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x92],
            address(None, Some(Ecx), 4, 0x9234_5678),
        ),
        (
            &[0x05, 0x78, 0x56, 0x34, 0x92],
            address(None, None, 1, 0x9234_5678),
        ),
        (
            &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
            address(Some(Esp), Some(Esi), 8, 0xf3f0_6766),
        ),
    ]
}

fn assert_operation(bytes: &[u8], address: EffectiveAddress, source: Register32) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::MemoryExchangeAddWord { address, source },
        "{bytes:02x?}"
    );
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_memory_xadd_all_sources_and_address_classes_have_exact_ir() {
    let mut rows = 0;
    for (ordinal, source) in REGISTERS.into_iter().enumerate() {
        for (tail, address) in addresses() {
            assert_operation(&instruction(ordinal as u8, tail), address, source);
            rows += 1;
        }
    }
    assert_eq!(rows, 104);
}

#[test]
fn word_memory_xadd_prefixes_payloads_and_neighbors_keep_precise_boundaries() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for source in 0..8 {
        let bytes = encoding(source);
        for prefix in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            let prefixed = [vec![prefix], bytes.clone()].concat();
            assert_eq!(
                decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                Some(unsupported),
                "{prefixed:02x?}"
            );
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let prefixed = [vec![prefix], bytes.clone()].concat();
            assert_eq!(
                decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Segment)),
                "{prefixed:02x?}"
            );
        }
        for opcode in [0xc0, 0xb0, 0xb1] {
            let closed = [0x66, 0x0f, opcode, 0x03 | source << 3];
            assert_eq!(
                decode_one(&code_space(CODE, &closed), GuestAddress(CODE)).err(),
                Some(unsupported),
                "{closed:02x?}"
            );
        }
        let raw = 0xc3 | source << 3;
        let register = [0x66, 0x0f, 0xc1, raw];
        assert_eq!(
            decode_one(&code_space(CODE, &register), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &Operation::ExchangeAddWord {
                destination: Register32::Ebx,
                source: REGISTERS[usize::from(source)],
            }
        );
        let locked_register = [0xf0, 0x66, 0x0f, 0xc1, raw];
        assert_eq!(
            decode_one(&code_space(CODE, &locked_register), GuestAddress(CODE)).err(),
            Some(DecodeError::InvalidEncoding)
        );
        for (opcode, is_byte) in [(0xc1, false), (0xc0, true)] {
            let neighbor = [0x0f, opcode, 0x03 | source << 3];
            let memory = code_space(CODE, &neighbor);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert!(
                if is_byte {
                    matches!(decoded.operation(), Operation::MemoryExchangeAddByte { .. })
                } else {
                    matches!(decoded.operation(), Operation::MemoryExchangeAdd { .. })
                },
                "{neighbor:02x?}"
            );
        }
    }
    assert_operation(
        &instruction(3, &[0x04, 0xdb]),
        EffectiveAddress {
            base: Some(Register32::Ebx),
            index: Some(Register32::Ebx),
            scale: 8,
            displacement: 0,
        },
        Register32::Ebx,
    );
    for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
        let index = (sib >> 3) & 7;
        let address = EffectiveAddress {
            base: Some(REGISTERS[usize::from(sib & 7)]),
            index: if index == 4 {
                None
            } else {
                Some(REGISTERS[usize::from(index)])
            },
            scale: 1 << (sib >> 6),
            displacement: 0,
        };
        assert_operation(&instruction(1, &[0x04, sib]), address, Register32::Ecx);
    }
}

#[test]
fn decoded_word_memory_xadd_stays_closed_in_standalone_backend() {
    for source in 0..8 {
        let mut bytes = encoding(source);
        bytes.extend([0xeb, 0]);
        let memory = code_space(CODE, &bytes);
        decode_one(&memory, GuestAddress(CODE)).unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(CODE),
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
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
    }
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: usize) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length: length as u32,
    }
}

#[test]
fn word_memory_xadd_fetch_checks_each_encoding_byte_and_top_boundary() {
    for source in 0..8 {
        let bytes = instruction(source, &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]);
        assert_eq!(bytes.len(), 9);
        for pc in [0x1fff, u32::MAX - bytes.len() as u32 + 1] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc()),
                (
                    bytes.len(),
                    GuestAddress(pc.wrapping_add(bytes.len() as u32))
                )
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let memory = code_space(pc, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1)),
                "source={source} cut={cut}"
            );
            let top = u32::MAX - cut as u32 + 1;
            let memory = code_space(top, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(top)).err(),
                Some(fetch_error(top, top, FaultReason::AddressOverflow, cut + 1))
            );
        }
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let mut memory = code_space(pc, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(
                pc,
                0x2000,
                FaultReason::Permission,
                bytes.len()
            ))
        );
    }
}

#[test]
fn word_memory_xadd_mixed_64_instruction_block_fits_every_bound_profile() {
    let mut bytes = Vec::new();
    for index in 0..32 {
        bytes.extend(encoding(index % 8));
    }
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_memory_xadd_late_refusal_preserves_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for source in 0..8 {
        bytes.extend(encoding(source));
    }
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0x0f, 0x0b]);
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&(CODE + 256).to_le_bytes());
    descriptor[4..].copy_from_slice(&2_u32.to_le_bytes());
    let replacement = engine.compile(1).unwrap();
    let resident = engine.compile_resident(1).unwrap().get();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let error = CompileError::Instruction {
        pc: GuestAddress(refused_pc),
        cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
            descriptor[4..]
                .copy_from_slice(&if entries { 0 } else { refused_length }.to_le_bytes());
            let before = engine.arena().to_vec();
            let result = match (is_resident, entries) {
                (false, false) => engine.compile(1).map(u64::from),
                (false, true) => engine.compile_entries(1, 0).map(u64::from),
                (true, false) => engine.compile_resident(1).map(|id| id.get()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
            };
            assert_eq!(
                result,
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), replacement_bytes);
            assert_eq!(
                engine.artifact_bytes().unwrap().as_ptr(),
                replacement_pointer
            );
            assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
            assert_eq!(
                engine.resident_bytes(resident).unwrap().as_ptr(),
                resident_pointer
            );
            engine.guard(KEY, replacement).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
        }
    }
}
