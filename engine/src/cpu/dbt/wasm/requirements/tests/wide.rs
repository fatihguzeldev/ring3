use super::{pair_tests::prepared, *};
use crate::{
    cpu::{
        dbt::{
            BlockSpec, CompilationTier32, CompileLimits, PreparedRegion,
            region::prepare_resident_region,
        },
        x86::{
            SegmentContext,
            decode::{CodeFetchContext, DecodeAuthority},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn exact(code: &[u8]) -> PreparedRegion {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), code).unwrap();
    prepare_resident_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: code.len() as u32,
        }],
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

#[test]
fn wide_import_requires_a_resident_run_longer_than_the_unchanged_v1_cap() {
    let resident = EmbeddedBinding::Resident { key: 7, id: 1 };
    let replacement = EmbeddedBinding::Replacement {
        key: 7,
        generation: 1,
    };
    for length in [1, 4, 8, 9, 16, 31, 32, 33] {
        let region = prepared(&[0xd9, 0xe0].repeat(length));
        for binding in [resident, replacement] {
            let plan = ModuleRequirements::new(&region.blocks, Some(binding));
            let expected = length >= 9 && matches!(binding, EmbeddedBinding::Resident { .. });
            assert_eq!(plan.has_x87_wide_groups, expected);
            let sections = crate::cpu::dbt::wasm::imports::wiring::build(&plan, Some(binding));
            assert_eq!(sections.context.x87_group32.is_some(), expected);
            assert_eq!(
                sections.context.x87_group.is_some(),
                length >= 4 && matches!(binding, EmbeddedBinding::Resident { .. })
            );
        }
    }
    let region = prepared(&[0x90; 32]);
    let plan = ModuleRequirements::new(&region.blocks, None);
    assert!(!plan.has_x87_wide_groups);
    assert!(
        crate::cpu::dbt::wasm::imports::wiring::build(&plan, None)
            .context
            .x87_group32
            .is_none()
    );
}

#[test]
fn wide_generated_lengths_contracts_and_scratch_compose_with_legacy_and_read_view() {
    let mut cases = [9, 16, 31, 32]
        .map(|length| [0xd9, 0xe0].repeat(length))
        .to_vec();
    cases.push([&[0xd9, 0x03][..], &[0xd9, 0xe0].repeat(7), &[0xd9, 0x1e]].concat());
    cases.push([&[0xd9, 0xe0].repeat(16)[..], &[0x8b, 0x03]].concat());
    let mut modules = Vec::new();
    for code in cases {
        let region = exact(&code);
        for binding in [
            EmbeddedBinding::Resident { key: 7, id: 1 },
            EmbeddedBinding::Replacement {
                key: 7,
                generation: 1,
            },
        ] {
            for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
                let bytes = crate::cpu::dbt::wasm::emit_with_tier(
                    &region.blocks,
                    Some(binding),
                    region.decode_key,
                    tier,
                );
                modules.push(format!(
                    "{{\"hex\":\"{}\",\"wide\":{}}}",
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                    matches!(binding, EmbeddedBinding::Resident { .. })
                ));
            }
        }
    }
    let script = "import assert from 'node:assert/strict'; let text=''; for await(const chunk of process.stdin) text+=chunk; const rows=JSON.parse(text); assert.equal(rows.length,24); for(const [i,row] of rows.entries()) { const bytes=Buffer.from(row.hex,'hex'); assert.ok(WebAssembly.validate(bytes),`wide module ${i}`); const imports=WebAssembly.Module.imports(new WebAssembly.Module(bytes)); assert.equal(imports.some(x=>x.name==='x87_resident_group32'),row.wide); assert.equal(imports.some(x=>x.name==='x87_resident_group'),row.wide); } console.log('24 wide group modules validated');";
    let mut child = Command::new("node")
        .args(["--input-type=module", "-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available for wide group validation");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", modules.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "24 wide group modules validated"
    );
}
