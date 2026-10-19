mod fixture;

use crate::{cpu::dbt::CompileLimits, cpu::x86::SegmentRegister, memory::GuestAddress};
use fixture::*;

#[test]
fn indirect_interior_hint_prepares_whole_optimized_owners_without_retiring_them() {
    let mut fixture = self::fixture::fixture();
    let before = retained(&fixture);
    let bytes = fixture.owners.map(|owner| {
        fixture
            .engine
            .resident_bytes(owner.unit_id)
            .unwrap()
            .to_vec()
    });
    let usage = fixture.engine.cache_usage().unwrap();
    let replacement = prepare(&mut fixture, limits(), TARGETS, None, None)
        .unwrap()
        .expect("seven material blocks fit and the indirect target is an old B instruction start");
    assert_eq!(
        replacement.coverage().owners(),
        &fixture.owners.map(|owner| owner.unit_id)
    );
    assert_eq!(fixture.engine.cache_usage().unwrap(), usage);
    assert_eq!(retained(&fixture), before);
    assert_eq!(
        fixture.owners.map(|owner| fixture
            .engine
            .resident_bytes(owner.unit_id)
            .unwrap()
            .to_vec()),
        bytes
    );
    assert_old_lookup(&fixture);
    drop(replacement);
    assert!(fixture.engine.pending_cache_request().unwrap().is_none());
}

#[test]
fn absent_unresolved_or_noninstruction_first_hint_cannot_retire_one_owner() {
    let mut fixture = fixture();
    let before = retained(&fixture);
    for target in [None, Some(0x7000), Some(0x2004)] {
        assert!(
            prepare(
                &mut fixture,
                limits(),
                [target, TARGETS[1], TARGETS[2]],
                None,
                None
            )
            .unwrap()
            .is_none()
        );
    }
    let root = fixture.owners[0].unit_id;
    assert!(
        prepare(&mut fixture, limits(), TARGETS, Some(root), None)
            .unwrap()
            .is_none()
    );
    assert_eq!(retained(&fixture), before);
    assert_old_lookup(&fixture);
}

#[test]
fn whole_owner_gate_and_pin_checks_allow_only_a_complete_selected_prefix() {
    let mut fixture = fixture();
    let before = retained(&fixture);
    let b = fixture.owners[1].unit_id;
    assert!(
        prepare(&mut fixture, limits(), TARGETS, Some(b), None)
            .unwrap()
            .is_none()
    );
    // the gate is a cold side entry, not the hinted b2003 entry.
    assert!(
        prepare(&mut fixture, limits(), TARGETS, None, Some(0x200e))
            .unwrap()
            .is_none()
    );
    let c = fixture.owners[2].unit_id;
    for (excluded, gate, targets) in [
        (Some(c), None, TARGETS),
        (None, Some(0x300b), TARGETS),
        (None, None, [TARGETS[0], Some(0x7000), TARGETS[2]]),
    ] {
        let replacement = prepare(&mut fixture, limits(), targets, excluded, gate)
            .unwrap()
            .expect("A+B remain complete when the next discovery lead is unavailable");
        assert_eq!(
            replacement.coverage().owners(),
            &fixture.owners[..2]
                .iter()
                .map(|o| o.unit_id)
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(retained(&fixture), before);
    assert_old_lookup(&fixture);
}

#[test]
fn configured_instruction_and_byte_limits_never_shorten_a_selected_owner() {
    let mut fixture = fixture();
    let before = retained(&fixture);
    // a has six instructions and b has six; eleven cannot select two whole owners.
    assert!(
        prepare(
            &mut fixture,
            CompileLimits {
                instructions: 11,
                ..limits()
            },
            TARGETS,
            None,
            None
        )
        .unwrap()
        .is_none()
    );
    let replacement = prepare(
        &mut fixture,
        CompileLimits {
            instructions: 12,
            ..limits()
        },
        TARGETS,
        None,
        None,
    )
    .unwrap()
    .expect("exact A+B prefix fits the configured instruction ceiling");
    assert_eq!(
        replacement.coverage().owners(),
        &fixture.owners[..2]
            .iter()
            .map(|o| o.unit_id)
            .collect::<Vec<_>>()
    );
    assert!(
        prepare(
            &mut fixture,
            CompileLimits {
                wasm_bytes: 1,
                ..limits()
            },
            TARGETS,
            None,
            None
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(retained(&fixture), before);
    assert_old_lookup(&fixture);
}

#[test]
fn stale_source_and_changed_decode_context_do_not_turn_hints_into_authority() {
    let mut fixture = self::fixture::fixture();
    let before = retained(&fixture);
    fixture
        .engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2ff0), &[1])
        .unwrap();
    assert!(
        prepare(&mut fixture, limits(), TARGETS, None, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(retained(&fixture), before);

    let mut fixture = self::fixture::fixture();
    let before = retained(&fixture);
    fixture
        .engine
        .set_segment_base(SegmentRegister::Cs, 0x4000)
        .unwrap();
    assert!(
        prepare(&mut fixture, limits(), TARGETS, None, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(retained(&fixture), before);
}
