use super::*;
use crate::{
    abi::arena::{self, ARENA_SIZE, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::{
        dbt::{
            BlockSpec, CompilationTier32, CompileLimits, PreparedRegion,
            region::prepare_embedded_region,
            wasm::{EmbeddedBinding, emitter::emit_with_tier},
        },
        x86::{SegmentCache, SegmentContext, SegmentRegister, decode::DecodeAuthority},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{Encode, Function};

const PC: u32 = 0x1000;

fn context() -> SegmentContext {
    let mut context = SegmentContext::default();
    let cs = SegmentRegister::Cs.index();
    context.caches[cs] = SegmentCache::from_bits(context.caches[cs].bits() & !(1 << 24)).unwrap();
    context
}

fn prepared(bytes: &[u8]) -> PreparedRegion {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(PC), bytes).unwrap();
    let cs = crate::cpu::x86::decode::CodeFetchContext::from_segments(&context()).unwrap();
    prepare_embedded_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(PC),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
        &[],
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn identical_dword_binary_uses_literal_zero_result_and_flags_without_generic_arithmetic() {
    for (opcode, kind) in [
        (0x31, BinaryKind::Xor),
        (0x29, BinaryKind::Sub),
        (0x39, BinaryKind::Cmp),
    ] {
        for encoding in 0..8 {
            let prepared = prepared(&[opcode, 0xc0 | encoding << 3 | encoding]);
            let instruction = &prepared.blocks[0].instructions[0];
            let Operation::Binary {
                kind: actual_kind,
                destination: Location32::Register(destination),
                source: Value32::Register(source),
            } = *instruction.operation()
            else {
                panic!("expected identical dword register operands");
            };
            assert_eq!((actual_kind, source), (kind, destination));
            let mut actual = Function::new([(27, ValType::I32)]);
            assert!(!emit(
                &mut actual.instructions(),
                instruction,
                Imports::default(),
                0,
            ));
            actual.instructions().end();
            let mut expected = Function::new([(27, ValType::I32)]);
            let mut code = expected.instructions();
            if kind != BinaryKind::Cmp {
                code.i32_const(0).local_set(register(destination));
            }
            code.local_get(FLAGS)
                .i32_const(!0x8d7)
                .i32_and()
                .i32_const(0x46)
                .i32_or()
                .local_set(FLAGS)
                .end();
            let mut before = Vec::new();
            let mut after = Vec::new();
            expected.encode(&mut before);
            actual.encode(&mut after);
            assert_eq!(
                after, before,
                "only the exact constant result and flag recipe"
            );
        }
    }
}

#[test]
fn generated_identical_dword_binary_preserves_full_state_and_precise_boundaries() {
    let mut arena = vec![0; ARENA_SIZE];
    arena::initialize(&mut arena).unwrap();
    let (records, bases) = arena.split_at_mut(SEGMENT_BASES_RECORD_OFFSET);
    crate::abi::x86::encode_current_segment_context(
        &context(),
        &mut records[SEGMENTS_OFFSET..SEGMENTS_OFFSET + crate::abi::x86::SEGMENTS_SIZE],
        bases,
    )
    .unwrap();
    let mut modules = Vec::new();
    for (opcode, name) in [(0x31, "xor"), (0x29, "sub"), (0x39, "cmp")] {
        for encoding in 0..8 {
            let prepared = prepared(&[opcode, 0xc0 | encoding << 3 | encoding]);
            for binding in [
                None,
                Some(EmbeddedBinding::Replacement {
                    key: 7,
                    generation: 1,
                }),
                Some(EmbeddedBinding::Resident { key: 7, id: 1 }),
            ] {
                for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
                    let bytes =
                        emit_with_tier(&prepared.blocks, binding, prepared.decode_key, tier);
                    modules.push(format!(
                        "{{\"name\":\"{name}\",\"register\":{encoding},\"bound\":{},\"bytes\":\"{}\"}}",
                        binding.is_some(),
                        hex(&bytes),
                    ));
                }
            }
        }
    }
    let input = format!(
        "{{\"arena\":\"{}\",\"segments\":{SEGMENTS_OFFSET},\"modules\":[{}]}}",
        hex(&arena),
        modules.join(","),
    );
    let mut child = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            include_str!("fixtures/runtime/run.mjs"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available for generated binary flag proof");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("identical dword binary controls passed (")
    );
}
