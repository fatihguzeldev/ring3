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

fn prepared(code: &[u8]) -> crate::cpu::dbt::PreparedRegion {
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
        &[0xd9, 0xe8, 0xd9, 0x1b],
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
