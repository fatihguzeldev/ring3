use crate::{
    abi::arena::{self, ARENA_SIZE, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::{
        dbt::{
            CompilationTier32, CompileLimits,
            cold::prepare_runtime_entry,
            wasm::{EmbeddedBinding, emitter::emit_with_tier, imports::Imports},
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

fn context() -> SegmentContext {
    let mut context = SegmentContext::default();
    let cs = SegmentRegister::Cs.index();
    context.caches[cs] = SegmentCache::from_bits(context.caches[cs].bits() & !(1 << 24)).unwrap();
    context
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn generated_direct_read_context_preserves_full_emitter_frontiers_and_untrusted_fallbacks() {
    assert!(Imports::default().read32_context_marker.is_none());
    let programs: [(&str, &[u8], &[u32]); 5] = [
        (
            "moves",
            &[0x8b, 0x03, 0x8b, 0x4b, 4, 0x89, 0xc6, 0xff, 0xe2],
            &[0, 2, 5, 7],
        ),
        (
            "fallback",
            &[0x8b, 0x03, 0x8b, 0x8b, 0xfd, 0x0f, 0, 0, 0x90, 0xff, 0xe2],
            &[0, 2, 8, 9],
        ),
        (
            "untrusted",
            &[
                0x8b, 0x03, 0x03, 0x8b, 0xfd, 0x0f, 0, 0, 0x8b, 0x33, 0xff, 0xe2,
            ],
            &[0, 2, 8, 10],
        ),
        ("last", &[0x90, 0x8b, 0x03], &[0, 1]),
        (
            "group",
            &[
                0x8b, 0x03, 0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0xe0, 0xd9, 0xe0, 0x8b, 0x33, 0xff, 0xe2,
            ],
            &[0, 2, 4, 6, 8, 10, 12],
        ),
    ];
    let mut modules = Vec::new();
    for (name, code, starts) in programs {
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
            CompileLimits {
                instructions: starts.len(),
                ..CompileLimits::default()
            },
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
        assert_eq!(prepared.blocks.len(), 1, "{name}: exact authored block");
        assert_eq!(
            prepared.blocks[0]
                .instructions
                .iter()
                .map(|instruction| instruction.pc().0 - PC)
                .collect::<Vec<_>>(),
            starts,
            "{name}: literal instruction starts"
        );
        for resident in [false, true] {
            if name == "group" && !resident {
                continue;
            }
            let binding = if resident {
                EmbeddedBinding::Resident { key: 7, id: 1 }
            } else {
                EmbeddedBinding::Replacement {
                    key: 7,
                    generation: 1,
                }
            };
            for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
                modules.push(format!(
                    "{{\"name\":\"{name}\",\"resident\":{resident},\"tier\":{},\"bytes\":\"{}\"}}",
                    tier as u32,
                    hex(&emit_with_tier(
                        &prepared.blocks,
                        Some(binding),
                        Some(cs.decode_key()),
                        tier,
                    ))
                ));
            }
        }
    }
    let mut arena = vec![0; ARENA_SIZE];
    arena::initialize(&mut arena).unwrap();
    let (records, bases) = arena.split_at_mut(SEGMENT_BASES_RECORD_OFFSET);
    crate::abi::x86::encode_current_segment_context(
        &context(),
        &mut records[SEGMENTS_OFFSET..SEGMENTS_OFFSET + crate::abi::x86::SEGMENTS_SIZE],
        bases,
    )
    .unwrap();
    let input = format!(
        "{{\"arena\":\"{}\",\"segments\":{SEGMENTS_OFFSET},\"bases\":{SEGMENT_BASES_RECORD_OFFSET},\"modules\":[{}]}}",
        hex(&arena),
        modules.join(","),
    );
    let mut child = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            concat!(
                include_str!("../../../../../../tests/support/wasm/run.mjs"),
                "\n",
                include_str!("fixtures/runtime/run.mjs"),
            ),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
        "full-emitter direct read context controls passed",
    );
}
