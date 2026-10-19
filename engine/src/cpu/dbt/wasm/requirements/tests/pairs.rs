use super::*;
use crate::{
    cpu::{
        dbt::{CompileLimits, cold::prepare_embedded_entry_region},
        x86::{
            SegmentContext,
            decode::{CodeFetchContext, DecodeAuthority},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

pub(super) fn prepared(code: &[u8]) -> crate::cpu::dbt::PreparedRegion {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let bytes = [code, &[0xff, 0xe2]].concat();
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1000)],
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

#[test]
fn pair_import_requires_resident_binding_and_two_eligible_adjacent_instructions() {
    let resident = EmbeddedBinding::Resident { key: 7, id: 1 };
    let replacement = EmbeddedBinding::Replacement {
        key: 7,
        generation: 1,
    };
    let pair = prepared(&[0xd9, 0xe8, 0xd9, 0xee]);
    assert!(ModuleRequirements::new(&pair.blocks, Some(resident)).has_x87_pairs);
    assert!(!ModuleRequirements::new(&pair.blocks, Some(replacement)).has_x87_pairs);
    // standalone planning accepts helper-free material, not computational x87.
    let unbound = prepared(&[0x90, 0x90]);
    let plan = ModuleRequirements::new(&unbound.blocks, None);
    assert!(!plan.has_x87_pairs);
    assert!(
        crate::cpu::dbt::wasm::imports::wiring::build(&plan, None)
            .context
            .x87_pair
            .is_none()
    );

    for code in [
        &[0x90, 0x90][..],
        &[0xd9, 0xe8],
        &[0xd9, 0xe8, 0x90, 0xd9, 0xee],
        &[0xd9, 0x1b, 0xd9, 0xe8],
        &[0xd9, 0xe8, 0xd9, 0x13],
        &[0xd9, 0xe8, 0xdd, 0x1b],
        &[0xd9, 0xe8, 0xdb, 0xf1],
        &[0xd9, 0xe8, 0xd9, 0x2b],
    ] {
        let region = prepared(code);
        let plan = ModuleRequirements::new(&region.blocks, Some(resident));
        assert!(!plan.has_x87_pairs, "{code:02x?}");
        let sections = crate::cpu::dbt::wasm::imports::wiring::build(&plan, Some(resident));
        assert!(sections.context.x87_pair.is_none());
    }
    let plan = ModuleRequirements::new(&pair.blocks, Some(resident));
    let sections = crate::cpu::dbt::wasm::imports::wiring::build(&plan, Some(resident));
    assert!(matches!(
        sections.context.x87_pair,
        Some(crate::cpu::dbt::wasm::imports::HelperBinding::Resident { key: 7, id: 1, .. })
    ));
}

#[test]
fn final_float32_store_pair_keeps_resident_only_import_and_write_validator() {
    let region = prepared(&[0xd9, 0xe8, 0xd9, 0x1b]);
    for binding in [
        EmbeddedBinding::Resident { key: 7, id: 1 },
        EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        },
    ] {
        let plan = ModuleRequirements::new(&region.blocks, Some(binding));
        assert!(plan.has_x87_write32_validator);
        assert_eq!(
            plan.has_x87_pairs,
            matches!(binding, EmbeddedBinding::Resident { .. })
        );
        let imports = crate::cpu::dbt::wasm::imports::wiring::build(&plan, Some(binding));
        assert_eq!(imports.context.x87_pair.is_some(), plan.has_x87_pairs);
    }
}

#[test]
fn group_import_requires_resident_binding_and_at_least_four_computational_steps() {
    let resident = EmbeddedBinding::Resident { key: 7, id: 1 };
    let replacement = EmbeddedBinding::Replacement {
        key: 7,
        generation: 1,
    };
    for length in 1..=8 {
        let region = prepared(&[0xd9, 0xe0].repeat(length));
        for binding in [resident, replacement] {
            let plan = ModuleRequirements::new(&region.blocks, Some(binding));
            let expected = length >= 4 && matches!(binding, EmbeddedBinding::Resident { .. });
            assert_eq!(plan.has_x87_groups, expected);
            let sections = crate::cpu::dbt::wasm::imports::wiring::build(&plan, Some(binding));
            assert_eq!(sections.context.x87_group.is_some(), expected);
            // existing two-step imports remain available for interior entries.
            assert_eq!(
                sections.context.x87_pair.is_some(),
                length >= 2 && matches!(binding, EmbeddedBinding::Resident { .. })
            );
        }
    }
    let unbound = prepared(&[0x90; 4]);
    let plan = ModuleRequirements::new(&unbound.blocks, None);
    assert!(!plan.has_x87_groups);
    assert!(
        crate::cpu::dbt::wasm::imports::wiring::build(&plan, None)
            .context
            .x87_group
            .is_none()
    );
}

#[test]
fn generated_group_lengths_contracts_and_terminal_labels_validate_in_both_tiers() {
    use crate::cpu::dbt::{BlockSpec, CompilationTier32, region::prepare_resident_region};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let mut cases = (4..=8)
        .map(|length| [0xd9, 0xe0].repeat(length))
        .collect::<Vec<_>>();
    cases.push(vec![0xd9, 0x03, 0xd9, 0xe0, 0xd9, 0x07, 0xd9, 0x1e]);
    let mut modules = Vec::new();
    for code in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &code).unwrap();
        let exact = prepare_resident_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: code.len() as u32,
            }],
            CompileLimits::default(),
            &[],
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
        for binding in [
            EmbeddedBinding::Resident { key: 7, id: 1 },
            EmbeddedBinding::Replacement {
                key: 7,
                generation: 1,
            },
        ] {
            for tier in [CompilationTier32::Baseline, CompilationTier32::Optimized] {
                let bytes = crate::cpu::dbt::wasm::emit_with_tier(
                    &exact.blocks,
                    Some(binding),
                    Some(cs.decode_key()),
                    tier,
                );
                modules.push(format!(
                    "{{\"hex\":\"{}\",\"group\":{}}}",
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                    matches!(binding, EmbeddedBinding::Resident { .. })
                ));
            }
        }
    }
    let mut child = Command::new("node").args(["--input-type=module", "-e", "import assert from 'node:assert/strict'; let text=''; for await(const chunk of process.stdin) text+=chunk; const rows=JSON.parse(text); assert.equal(rows.length,24); for(const [i,row] of rows.entries()) { const bytes=Buffer.from(row.hex,'hex'); assert.ok(WebAssembly.validate(bytes),`module ${i}`); const module=new WebAssembly.Module(bytes); assert.equal(WebAssembly.Module.imports(module).some(x=>x.name==='x87_resident_group'),row.group); } console.log('24 group modules validated');"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("node available for generated x87 group validation");
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
        "24 group modules validated"
    );
}
