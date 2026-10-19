use super::*;

#[test]
fn generated_x87_write32_matches_inline_smc_faults_scratch_and_live_context() {
    let input = format!(
        "{{\"module\":\"{}\",\"segments\":{},\"bases\":{}}}",
        hex(&differential_span_module(4, 2, true)),
        SEGMENTS_OFFSET,
        SEGMENT_BASES_RECORD_OFFSET,
    );
    let output = node(include_str!("../fixtures/runtime/write32.mjs"), &input);
    assert!(output.starts_with("x87 write32 inline/outlined differential controls passed ("));
    print!("{output}");
}

#[test]
fn write32_outline_requires_the_x87_ordinary_store_contract() {
    for (length, access, smc, paging, intent, has_helper) in [
        (0, 0, false, true, None, true),
        (2, 2, true, true, None, true),
        (8, 2, true, true, None, true),
        (10, 2, true, true, None, true),
        (4, 1, true, true, None, true),
        (4, 3, true, true, None, true),
        (4, 2, false, true, None, true),
        (4, 2, true, false, None, true),
        (4, 2, true, true, Some(1), true),
        (4, 2, true, true, Some(2), true),
        (4, 2, true, true, None, false),
    ] {
        let context = ResultContext {
            paging_configured: paging,
            paging_access_mask: intent,
            zero_span_validator: None,
            x87_read32_validator: None,
            x87_write32_validator: has_helper.then_some(42),
        };
        let mut actual = Function::new([(36, ValType::I32)]);
        let mut retained = Function::new([(36, ValType::I32)]);
        validate_x87(&mut actual.instructions(), length, access, smc, 0, context);
        validate(
            &mut retained.instructions(),
            length,
            access,
            smc,
            0,
            context,
        );
        let mut before = Vec::new();
        let mut after = Vec::new();
        retained.encode(&mut before);
        actual.encode(&mut after);
        assert_eq!(
            after, before,
            "nonmatching x87 contract stays byte-identical"
        );
    }
    let context = ResultContext {
        paging_configured: true,
        paging_access_mask: None,
        zero_span_validator: None,
        x87_read32_validator: None,
        x87_write32_validator: Some(42),
    };
    let mut actual = Function::new([(36, ValType::I32)]);
    let mut retained = Function::new([(36, ValType::I32)]);
    validate(&mut actual.instructions(), 4, 2, true, 0, context);
    validate_span(
        &mut retained.instructions(),
        4,
        2,
        true,
        Validation::Ordinary { mask: None },
        0,
        context,
    );
    let mut before = Vec::new();
    let mut after = Vec::new();
    retained.encode(&mut before);
    actual.encode(&mut after);
    assert_eq!(
        after, before,
        "non-x87 stores keep their original validator"
    );
}

#[test]
fn write32_planning_counts_waiting_sites_and_composes_with_all_private_validators() {
    use crate::{
        cpu::{
            dbt::{
                CompileLimits,
                cold::prepare_runtime_entry,
                wasm::{EmbeddedBinding, emitter::emit, requirements::ModuleRequirements},
            },
            x86::{
                SegmentContext,
                decode::{CodeFetchContext, DecodeAuthority},
            },
        },
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    let cases: [(&[u8], usize); 7] = [
        (&[0x90, 0xff, 0xe2], 0),
        (&[0xd9, 0x18, 0xff, 0xe2], 2), // fstp float32.
        (&[0xdb, 0x18, 0xff, 0xe2], 2), // fistp int32.
        (&[0xdd, 0x18, 0xff, 0xe2], 0), // fstp float64.
        (&[0xdd, 0xd8, 0xff, 0xe2], 0), // fstp register.
        (
            &[
                0xd9, 0x00, 0xd9, 0xe8, 0xd9, 0x18, 0x8b, 0x03, 0x8b, 0x0b, 0xff, 0xe2,
            ],
            2,
        ),
        (&[0xd9, 0xe8, 0xd9, 0x18, 0xff, 0xe2], 2), // fld1; fstp float32.
    ];
    let mut modules = Vec::new();
    for (index, (code, sites)) in cases.into_iter().enumerate() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), code).unwrap();
        let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
        let prepared = prepare_runtime_entry(
            &memory,
            GuestAddress(0x1000),
            CompileLimits::default(),
            Some(cs),
            DecodeAuthority::new(None),
        )
        .unwrap();
        for binding in [
            EmbeddedBinding::Replacement {
                key: 7,
                generation: 1,
            },
            EmbeddedBinding::Resident { key: 7, id: 1 },
        ] {
            let plan = ModuleRequirements::new(&prepared.blocks, Some(binding));
            assert_eq!(plan.has_x87_write32_validator, sites >= 2);
            if index == 5 {
                assert!(
                    plan.has_segment_check
                        && plan.has_zero_span_validator
                        && plan.has_x87_read32_validator
                        && plan.has_read32_validator
                );
            }
            take_write32_sites();
            let bytes = emit(&prepared.blocks, Some(binding), Some(cs.decode_key()));
            let emitted_sites = if index == 6 && matches!(binding, EmbeddedBinding::Resident { .. })
            {
                3
            } else {
                sites
            };
            assert_eq!(
                take_write32_sites(),
                emitted_sites,
                "waiting, reported final-pair store and ordinary side entry are real sites"
            );
            modules.push(format!("\"{}\"", hex(&bytes)));
        }
    }
    let script = "import assert from 'node:assert/strict'; let input=''; for await(const c of process.stdin) input+=c; const modules=JSON.parse(input); assert.equal(modules.length,14); for(const [i,hex] of modules.entries()) assert.ok(WebAssembly.validate(Buffer.from(hex,'hex')),`module ${i}`); console.log('14 write32 composition modules validated');";
    assert_eq!(
        node(script, &format!("[{}]", modules.join(","))).trim(),
        "14 write32 composition modules validated"
    );
}
