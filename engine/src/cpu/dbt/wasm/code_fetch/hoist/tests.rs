use crate::{
    abi::arena::{self, ARENA_SIZE, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::{
        dbt::{
            CompilationTier32, CompileLimits,
            cold::prepare_runtime_entry,
            wasm::{EmbeddedBinding, code_fetch, emitter::emit_with_tier},
        },
        x86::{SegmentCache, SegmentContext, SegmentRegister, decode::DecodeAuthority},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const PC: u32 = 0x1000;
const X87: &[u8] = &[
    0x90, 0x90, 0x90, 0xd9, 0xe8, 0x89, 0xc1, 0x90, 0x90, 0xff, 0xe2,
];
const READ: &[u8] = &[
    0x90, 0x90, 0x90, 0x8b, 0x03, 0x89, 0xc1, 0x90, 0x90, 0xff, 0xe2,
];

fn context() -> SegmentContext {
    let mut context = SegmentContext::default();
    let cs = SegmentRegister::Cs.index();
    context.caches[cs] = SegmentCache::from_bits(context.caches[cs].bits() & !(1 << 24)).unwrap();
    context
}

fn module(code: &[u8], tier: CompilationTier32, resident: bool) -> Vec<u8> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(PC), code).unwrap();
    let cs = crate::cpu::x86::decode::CodeFetchContext::from_segments(&context()).unwrap();
    let prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(PC),
        CompileLimits::default(),
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap();
    let binding = if resident {
        EmbeddedBinding::Resident { key: 7, id: 1 }
    } else {
        EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        }
    };
    emit_with_tier(&prepared.blocks, Some(binding), Some(cs.decode_key()), tier)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn generated_mixed_blocks_preserve_side_entries_and_import_mutation_order() {
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
    for (name, code) in [("x87", X87), ("read", READ)] {
        for resident in [false, true] {
            for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
                modules.push(format!(
                    "{{\"name\":\"{name}\",\"resident\":{resident},\"tier\":{},\"bytes\":\"{}\"}}",
                    tier as u32,
                    hex(&module(code, tier, resident))
                ));
            }
        }
    }
    let input = format!(
        "{{\"arena\":\"{}\",\"segments\":{SEGMENTS_OFFSET},\"bases\":{SEGMENT_BASES_RECORD_OFFSET},\"modules\":[{}]}}",
        hex(&arena),
        modules.join(",")
    );
    if let Some(path) = std::env::var_os("RING3_CONTEXT_CONTROL_OUTPUT") {
        std::fs::write(path, &input).unwrap();
    }
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
        .expect("node available for generated context barrier proof");
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
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "mixed context barrier controls passed"
    );
}

#[test]
fn generated_refresh_count_tracks_import_barriers_instead_of_every_instruction() {
    // bound modules without avx use local 31 for their code-fetch limit.
    let mut signature = wasm_encoder::Function::new([]);
    code_fetch::refresh(&mut signature.instructions(), 31);
    let signature = signature.into_raw_body();
    let signature = &signature[1..];
    let pure = [vec![0x90; 7], vec![0xff, 0xe2]].concat();
    let dense = [vec![0xd9, 0xe8].repeat(7), vec![0xff, 0xe2]].concat();
    for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
        for (code, expected) in [
            (pure.as_slice(), 2),
            (X87, 3),
            (READ, 3),
            (dense.as_slice(), 9),
        ] {
            let bytes = module(code, tier, true);
            assert_eq!(
                bytes
                    .windows(signature.len())
                    .filter(|window| *window == signature)
                    .count(),
                expected
            );
        }
    }
}
