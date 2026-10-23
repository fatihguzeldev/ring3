use super::*;
use crate::cpu::core::program;
use crate::cpu::x86::{
    Register32, X87Binary, X87Format,
    ir::{BinaryKind, BranchTarget, Location32, Operation, Value32},
};
use crate::memory::GuestAddress;

fn block(pc: u32, operations: &[Operation]) -> program::Block {
    program::Block {
        entry: pc,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(i, operation)| program::Instruction {
                pc: pc + i as u32,
                next: pc + i as u32 + 1,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    }
}
fn mov(register: Register32, value: u32) -> Operation {
    Operation::Move {
        destination: Location32::Register(register),
        source: Value32::Immediate(value),
    }
}
fn jump(pc: u32) -> Operation {
    Operation::Jump {
        target: BranchTarget::Direct(GuestAddress(pc)),
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
fn single(operations: &[Operation]) -> RegionPlan {
    RegionPlan::analyze(&Region::from_blocks(&[block(0x1000, operations)], 0x1000).unwrap())
}
fn binary() -> Operation {
    fp(X87Operation::Binary {
        kind: X87Binary::Add,
        destination: 0,
        source: 1,
        pop: false,
    })
}

#[test]
fn scalar_copies_constants_and_wrapped_status_output_have_distinct_versions() {
    let plan = single(&[
        mov(Register32::Eax, 7),
        Operation::Move {
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Register(Register32::Eax),
        },
        fp(X87Operation::StatusToAx),
        Operation::Binary {
            kind: BinaryKind::Add,
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Immediate(2),
        },
    ]);
    let after_copy = plan.frame(plan.instruction(BlockId(0), 1).after);
    assert_eq!(after_copy.gpr[0], after_copy.gpr[3]);
    let after_status = plan.frame(plan.instruction(BlockId(0), 2).after);
    assert_eq!(plan.constant(after_status.gpr[0]), None);
    assert_eq!(plan.constant(after_status.gpr[3]), Some(7));
    assert_eq!(
        plan.constant(plan.frame(plan.block(BlockId(0)).exit).gpr[3]),
        Some(9)
    );
}

#[test]
fn internal_backedge_phi_never_removes_the_external_entry_guard() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(0x2000, &[Operation::Nop, jump(0x2000)]),
    ];
    let region = Region::from_blocks(&blocks, 0x1000).unwrap();
    let plan = RegionPlan::analyze(&region);
    let entry = &plan.block(BlockId(1)).contract;
    assert!(entry.external);
    assert_eq!(entry.gpr_constants[0], Some(19));
    let ValueKind::Phi { block, inputs } = &plan.value(plan.frame(entry.frame).gpr[0]).kind else {
        panic!("backedge requires a phi")
    };
    assert_eq!(*block, BlockId(1));
    assert_eq!(
        inputs.iter().map(|(block, _)| *block).collect::<Vec<_>>(),
        [BlockId(0), BlockId(1)]
    );
    // Source versions are stable arena IDs, not re-lowered instruction copies.
    assert_eq!(inputs[1].1, plan.frame(plan.block(BlockId(1)).exit).gpr[0]);
    let rerun = RegionPlan::analyze(&region);
    assert_eq!(plan.frames(), rerun.frames());
}

#[test]
fn secondary_constant_contract_requires_imported_register_equality() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    assert!(contract.external);
    assert_eq!(contract.gpr_constants[0], Some(19));
    assert_eq!(plan.constant(plan.frame(contract.frame).gpr[0]), Some(19));
}

#[test]
fn loop_admissions_and_occupancy_are_guarded_at_public_entry() {
    let blocks = [
        block(0x1000, &[binary(), jump(0x2000)]),
        block(0x2000, &[binary(), jump(0x2000)]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    assert!(contract.external);
    assert!(
        contract
            .guards
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::Control)
    );
    for index in 0..2 {
        let values = plan.frame(contract.frame).fp.payload[index].values();
        assert!(
            contract
                .guards
                .iter()
                .any(|&id| plan.proof(id).kind == ProofKind::Occupied
                    && plan.proof(id).value.is_some_and(|v| values.contains(&v)))
        );
        assert!(matches!(
            plan.frame(contract.frame).fp.payload[index],
            FpValue::Tagged { .. }
        ));
    }
}

#[test]
fn modifying_backedge_kills_a_preheader_constant_at_the_join() {
    let blocks = [
        block(0x1000, &[mov(Register32::Eax, 19), jump(0x2000)]),
        block(
            0x2000,
            &[
                Operation::Binary {
                    kind: BinaryKind::Add,
                    destination: Location32::Register(Register32::Eax),
                    source: Value32::Immediate(1),
                },
                jump(0x2000),
            ],
        ),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    assert_eq!(plan.block(BlockId(1)).contract.gpr_constants[0], None);
}

#[test]
fn stack_renaming_retains_all_ghosts_and_uses_at_most_eight_cells() {
    let plan = single(&[
        fp(X87Operation::LoadRegister { index: 1 }),
        fp(X87Operation::Exchange { index: 2 }),
        fp(X87Operation::StoreRegister {
            index: 1,
            pop: true,
        }),
    ]);
    let entry = plan.frame(plan.block(BlockId(0)).entry);
    let pushed = plan.frame(plan.instruction(BlockId(0), 0).after);
    assert_eq!(pushed.fp.payload[0], entry.fp.payload[1]);
    assert_eq!(pushed.fp.payload[1], entry.fp.payload[0]);
    // Only the architecturally overwritten push destination disappears.
    for old in &entry.fp.payload[..7] {
        assert!(pushed.fp.payload.contains(old));
    }
    let final_frame = plan.frame(plan.block(BlockId(0)).exit);
    assert_eq!(final_frame.fp.tags[7], TagFact::Empty);
    assert_eq!(final_frame.fp.payload[7], final_frame.fp.payload[0]);
    for frame in plan.frames() {
        assert_eq!(frame.fp.payload.len(), 8);
        for payload in frame.fp.payload {
            for value in payload.values() {
                assert!(plan.storage(value).unwrap().cell.0 < 8);
            }
        }
    }
}

#[test]
fn helper_and_control_change_kill_all_previous_admission_proofs() {
    let plan = single(&[binary(), fp(X87Operation::LoadControl), binary()]);
    let first = plan.instruction(BlockId(0), 0);
    let helper = plan.instruction(BlockId(0), 1);
    let resumed = plan.instruction(BlockId(0), 2);
    assert!(plan.proofs_at(first.before).is_empty());
    assert!(
        plan.proofs_at(first.after)
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert_ne!(
        plan.frame(first.after).fp.control_epoch,
        plan.frame(helper.after).fp.control_epoch
    );
    assert!(plan.proofs_at(resumed.before).is_empty());
    assert!(
        plan.frame(helper.after)
            .fp
            .payload
            .iter()
            .all(|payload| matches!(payload, FpValue::Tagged { .. }))
    );
    assert!(helper.fp.as_ref().unwrap().kills_proofs);
}

#[test]
fn success_proofs_do_not_appear_at_the_faulting_prefix() {
    let plan = single(&[
        fp(X87Operation::Load(X87Format::Float32)),
        binary(),
        binary(),
    ]);
    let load = plan.instruction(BlockId(0), 0);
    let first = plan.instruction(BlockId(0), 1);
    let second = plan.instruction(BlockId(0), 2);
    assert!(plan.proofs_at(load.before).is_empty());
    assert!(
        plan.proofs_at(first.before)
            .iter()
            .any(|&id| plan.proof(id).kind == ProofKind::Native24)
    );
    let source = plan.frame(first.after).fp.payload[1].values()[0];
    assert!(
        !plan
            .proofs_at(first.before)
            .iter()
            .any(|&id| plan.proof(id).value == Some(source)
                && plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert!(
        plan.proofs_at(second.before)
            .iter()
            .any(|&id| plan.proof(id).value == Some(source)
                && plan.proof(id).kind == ProofKind::BoundedF64)
    );
    assert_eq!(plan.frame(first.before).pc, ResumePc::Exact(0x1001));
    assert_eq!(plan.frame(first.before).retired, 1);
    assert_eq!(plan.frame(first.after).pc, ResumePc::Exact(0x1002));
    assert_eq!(plan.frame(first.after).retired, 2);
}

#[test]
fn shape_interning_excludes_instruction_metadata() {
    let plan = single(&[fp(X87Operation::ChangeSign), fp(X87Operation::Absolute)]);
    let first = plan.instruction(BlockId(0), 0).after;
    let second = plan.instruction(BlockId(0), 1).after;
    assert_eq!(plan.shape_id(first), plan.shape_id(second));
    assert_ne!(
        plan.metadata(first).instruction,
        plan.metadata(second).instruction
    );
}

#[test]
fn identity_layout_with_changed_dirty_or_tags_still_rebases_at_edge() {
    let blocks = [
        block(0x1000, &[fp(X87Operation::ChangeSign), jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let edge = &plan.block(BlockId(0)).edges[0];
    assert_eq!(
        plan.shape(edge.frame).cells,
        plan.shape(plan.block(edge.target).entry).cells
    );
    assert!(edge.fp_rebase);
    assert_eq!(edge.frame, plan.block(BlockId(0)).exit);
}

#[test]
fn actual_simd_fast_success_preserves_the_renamed_fp_frame() {
    use crate::cpu::x86::simd::{FloatKind, SimdKind, SimdOperand, SimdOperation, SimdRegister};
    let move_op = SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    };
    let add_op = SimdOperation {
        kind: SimdKind::Float(FloatKind::Add),
        lane_bytes: 4,
        ..move_op
    };
    for operation in [move_op, add_op] {
        let plan = single(&[
            fp(X87Operation::LoadRegister { index: 1 }),
            Operation::Simd(operation),
            binary(),
        ]);
        let instruction = plan.instruction(BlockId(0), 1);
        assert_eq!(
            plan.frame(instruction.before).fp,
            plan.frame(instruction.after).fp,
            "the actual direct SIMD success keeps every x87 value, tag, epoch and metadata recipe"
        );
        assert_eq!(
            plan.shape(instruction.before),
            plan.shape(instruction.after)
        );
        assert_ne!(plan.shape(instruction.after).top_delta, 0);
        assert_eq!(plan.instruction(BlockId(0), 2).before, instruction.after);
    }
    for operation in [
        SimdOperation {
            scalar: true,
            ..add_op
        },
        SimdOperation {
            destination: SimdOperand::Register(SimdRegister::Mmx(0)),
            source: SimdOperand::Register(SimdRegister::Mmx(1)),
            vector_bytes: 8,
            lane_bytes: 8,
            ..move_op
        },
    ] {
        let plan = single(&[
            fp(X87Operation::LoadRegister { index: 1 }),
            Operation::Simd(operation),
        ]);
        let instruction = plan.instruction(BlockId(0), 1);
        assert_ne!(
            plan.frame(instruction.before).fp.control_epoch,
            plan.frame(instruction.after).fp.control_epoch
        );
        assert_eq!(plan.shape(instruction.after).top_delta, 0);
    }
}

#[test]
fn actual_simd_fast_success_carries_fp_admission_facts_across_cfg_edges() {
    use crate::cpu::x86::simd::{SimdKind, SimdOperand, SimdOperation, SimdRegister};
    let operation = Operation::Simd(SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    });
    let blocks = [
        block(0x1000, &[binary(), operation, jump(0x2000)]),
        block(0x2000, &[Operation::Nop]),
    ];
    let plan = RegionPlan::analyze(&Region::from_blocks(&blocks, 0x1000).unwrap());
    let contract = &plan.block(BlockId(1)).contract;
    for kind in [ProofKind::Control, ProofKind::Occupied, ProofKind::ExactF64] {
        assert!(
            contract
                .guards
                .iter()
                .any(|&id| plan.proof(id).kind == kind)
        );
    }
}
