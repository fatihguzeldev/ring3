use super::support_fetch as fetch;
use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::encode_current_segment_context,
    },
    cpu::{
        UnsupportedFeature,
        x86::{
            SegmentCache,
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

const PC: u32 = 0x1100;
const FORMS: &[&[u8]] = &[
    &[0xf3, 0xc3],
    &[0xf3, 0xc2, 0x66, 0xf3],
    &[0x66, 0xf3, 0xc3],
    &[0xf3, 0x66, 0xc3],
    &[0x66, 0xf3, 0xc2, 0x66, 0xf3],
    &[0xf3, 0x66, 0xc2, 0x66, 0xf3],
];

fn code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn rep_near_return_uses_existing_width_and_unsigned_adjustment() {
    for bytes in FORMS {
        let memory = code(PC, bytes);
        let decoded = decode_one(&memory, GuestAddress(PC)).unwrap();
        let prefix_length = bytes
            .iter()
            .take_while(|byte| matches!(**byte, 0x66 | 0xf3))
            .count();
        let word = bytes[..prefix_length].contains(&0x66);
        let stack_adjust = if bytes[prefix_length] == 0xc2 {
            0xf366
        } else {
            0
        };
        let expected = if word {
            Operation::ReturnWord { stack_adjust }
        } else {
            Operation::Return { stack_adjust }
        };
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (bytes.len(), GuestAddress(PC + bytes.len() as u32))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn rep_near_return_compiles_with_each_code_default_and_owner() {
    for bytes in FORMS {
        for d16 in [false, true] {
            for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut engine = EngineInstance::new(1, 0x5245_5052_4554).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                let source = [*bytes, &[0xf0, 0x90]].concat();
                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + source.len()]
                    .copy_from_slice(&source);
                engine.upload(PC, source.len() as u32).unwrap();
                if d16 {
                    let mut context = engine.segment_context().unwrap();
                    context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                    let mut segments = [0; 64];
                    let mut bases = [0; 40];
                    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
                    let arena = engine.arena_mut().unwrap();
                    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&segments);
                    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40]
                        .copy_from_slice(&bases);
                }
                let arena = engine.arena_mut().unwrap();
                arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&PC.to_le_bytes());
                if !entries {
                    arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                        .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                }
                let before = engine.arena().to_vec();
                let mut ram = [0; 4096];
                engine
                    .memory()
                    .unwrap()
                    .read(GuestAddress(0x1000), &mut ram)
                    .unwrap();
                let id = match (resident, entries) {
                    (false, false) => engine.compile(1).map(|_| None),
                    (false, true) => engine.compile_entries(1, 0).map(|_| None),
                    (true, false) => engine.compile_resident(1).map(|id| Some(id.get())),
                    (true, true) => engine
                        .compile_resident_entries(1, 0)
                        .map(|id| Some(id.get())),
                }
                .expect("compiler-emitted rep ret uses ordinary near-return semantics");
                let module = id.map_or_else(
                    || engine.artifact_bytes().unwrap(),
                    |id| engine.resident_bytes(id).unwrap(),
                );
                assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
                assert_eq!(engine.arena(), before);
                let mut after = [0; 4096];
                engine
                    .memory()
                    .unwrap()
                    .read(GuestAddress(0x1000), &mut after)
                    .unwrap();
                assert_eq!(after, ram);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            }
        }
    }
}

#[test]
fn rep_return_keeps_duplicate_lock_far_and_iret_boundaries() {
    for bytes in [
        &[0xf3, 0xf3, 0xc3][..],
        &[0xf2, 0xf3, 0xc3],
        &[0xf3, 0xf2, 0xc3],
        &[0x66, 0x66, 0xf3, 0xc3],
        &[0xf3, 0xcb],
        &[0xf3, 0xca, 0, 0],
        &[0xf3, 0xcf],
    ] {
        assert_eq!(
            decode_one(&code(PC, bytes), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
            "{bytes:02x?}"
        );
    }
    for bytes in [&[0xf0, 0xf3, 0xc3][..], &[0xf3, 0xf0, 0xc2, 0, 0]] {
        assert_eq!(
            decode_one(&code(PC, bytes), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn rep_return_fetches_the_complete_prefix_opcode_and_immediate() {
    for bytes in FORMS {
        let pc = 0x2000 - bytes.len() as u32;
        assert_eq!(
            decode_one(&code(pc, bytes), GuestAddress(pc))
                .unwrap()
                .length() as usize,
            bytes.len()
        );
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            fetch::assert_error(
                decode_one(&code(pc, &bytes[..cut]), GuestAddress(pc)).unwrap_err(),
                pc,
                0x2000,
                FaultReason::Unmapped,
                cut as u32 + 1,
            );
        }
    }
}
