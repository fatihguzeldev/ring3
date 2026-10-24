use super::*;
use crate::cpu::core::program;
use crate::cpu::x86::ir::{BranchTarget, Operation};
use crate::memory::GuestAddress;

fn block(pc: u32, operations: &[Operation]) -> program::Block {
    program::Block {
        entry: pc,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(index, operation)| program::Instruction {
                pc: pc + index as u32,
                next: pc + index as u32 + 1,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    }
}

fn fp(operation: X87Operation) -> Operation {
    Operation::X87 {
        operation,
        address: None,
        waiting: false,
        opcode: 0x123,
    }
}

fn jump(pc: u32) -> Operation {
    Operation::Jump {
        target: BranchTarget::Direct(GuestAddress(pc)),
    }
}

fn multiply() -> Operation {
    fp(X87Operation::Binary {
        kind: X87Binary::Multiply,
        destination: 0,
        source: 1,
        pop: false,
    })
}

#[test]
fn prepared_native_values_have_an_explicit_epoch_renaming_but_external_use_stays_guarded() {
    let load = fp(X87Operation::Load(X87Format::Float32));
    let graph = Region::from_blocks(
        &[
            block(0x1000, &[load, load, jump(0x2000)]),
            block(0x2000, &[multiply()]),
        ],
        0x1000,
    )
    .unwrap();
    let values = RegionPlan::analyze(&graph);
    let guards = Guards::analyze(&graph, &values);
    let edge = guards.continuation(BlockId(0), BlockId(1));
    assert_eq!(edge.source_frame, values.block(BlockId(0)).exit);
    assert_eq!(edge.target_frame, values.block(BlockId(1)).entry);
    assert_eq!(
        edge.control.source,
        values.frame(edge.source_frame).fp.control_epoch
    );
    assert_eq!(
        edge.control.target,
        values.frame(edge.target_frame).fp.control_epoch
    );
    assert_ne!(edge.control.source, edge.control.target);
    let target = guards.at(edge.target_frame);
    assert!(target.enter && target.control && !target.exact_mul);
    assert!(target.inputs.iter().all(|input| {
        input.occupied && input.load && input.admit == Some(ProofKind::Native24InSingle)
    }));
}

#[test]
fn payload_aliases_and_independent_destination_tags_have_different_continuation_maps() {
    let graph = Region::from_blocks(
        &[
            block(
                0x1000,
                &[fp(X87Operation::LoadRegister { index: 0 }), jump(0x2000)],
            ),
            block(0x2000, &[fp(X87Operation::Exchange { index: 1 })]),
        ],
        0x1000,
    )
    .unwrap();
    let values = RegionPlan::analyze(&graph);
    let guards = Guards::analyze(&graph, &values);
    let edge = guards.continuation(BlockId(0), BlockId(1));
    assert_eq!(edge.payload_sources[0], edge.payload_sources[1]);
    assert_eq!(
        edge.tag_sources[0],
        TagRecipe::Value(edge.payload_sources[0])
    );
    assert_eq!(edge.tag_sources[1], TagRecipe::Entry(0));
    assert_ne!(edge.tag_sources[0], edge.tag_sources[1]);
}

#[test]
fn a_control_barrier_maps_the_new_epoch_and_does_not_recover_old_native24_facts() {
    let load = fp(X87Operation::Load(X87Format::Float32));
    let graph = Region::from_blocks(
        &[
            block(0x1000, &[load, load, jump(0x2000)]),
            block(0x2000, &[fp(X87Operation::LoadControl), jump(0x3000)]),
            block(0x3000, &[multiply()]),
        ],
        0x1000,
    )
    .unwrap();
    let values = RegionPlan::analyze(&graph);
    let guards = Guards::analyze(&graph, &values);
    let edge = guards.continuation(BlockId(1), BlockId(2));
    assert_ne!(
        edge.control.source,
        values
            .frame(values.block(BlockId(1)).entry)
            .fp
            .control_epoch
    );
    let target = guards.at(edge.target_frame);
    assert!(!target.exact_mul);
    assert!(
        target
            .inputs
            .iter()
            .all(|input| { input.admit == Some(ProofKind::Native24InSingle) })
    );
}

#[test]
fn a_join_with_an_unknown_predecessor_does_not_promote_its_payload_contract() {
    let load = fp(X87Operation::Load(X87Format::Float32));
    let graph = Region::from_blocks(
        &[
            block(0x1000, &[load, load, jump(0x3000)]),
            block(0x2000, &[jump(0x3000)]),
            block(0x3000, &[multiply()]),
        ],
        0x1000,
    )
    .unwrap();
    let values = RegionPlan::analyze(&graph);
    let guards = Guards::analyze(&graph, &values);
    let target = guards.at(values.block(BlockId(2)).entry);
    assert!(!target.exact_mul);
    assert!(
        target
            .inputs
            .iter()
            .all(|input| { input.admit == Some(ProofKind::Native24InSingle) })
    );
    assert_ne!(
        guards.continuation(BlockId(0), BlockId(2)).payload_sources,
        guards.continuation(BlockId(1), BlockId(2)).payload_sources
    );
}

#[test]
fn sign_changes_keep_payload_identity_without_licensing_external_entry_hints() {
    let load = fp(X87Operation::Load(X87Format::Float32));
    for operation in [X87Operation::ChangeSign, X87Operation::Absolute] {
        let graph = Region::from_blocks(
            &[
                block(0x1000, &[load, load, fp(operation), jump(0x2000)]),
                block(0x2000, &[multiply()]),
            ],
            0x1000,
        )
        .unwrap();
        let values = RegionPlan::analyze(&graph);
        let guards = Guards::analyze(&graph, &values);
        let sign = values.instruction(BlockId(0), 2);
        assert_ne!(
            PayloadKey::from(values.frame(sign.before).fp.payload[0]),
            PayloadKey::from(values.frame(sign.after).fp.payload[0])
        );
        assert_eq!(
            values.frame(sign.before).fp.control_epoch,
            values.frame(sign.after).fp.control_epoch
        );
        assert!(!guards.at(values.block(BlockId(1)).entry).exact_mul);
        let edge = guards.continuation(BlockId(0), BlockId(1));
        assert_eq!(
            edge.payload_sources[0],
            values.fp_output_cell(sign.after).unwrap()
        );
        assert!(matches!(edge.tag_sources[0], TagRecipe::Value(_)));
    }
}
