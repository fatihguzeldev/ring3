use super::*;
use crate::cpu::{
    core::program,
    x86::{
        Register32, SegmentRegister, X87Binary,
        ir::{UnaryKind, Value32},
        simd::{SimdKind, SimdOperation, SimdRegister},
    },
};

fn plan(operations: &[Operation]) -> RegionPlan {
    let block = program::Block {
        entry: 0x1000,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(i, operation)| program::Instruction {
                pc: 0x1000 + i as u32,
                next: 0x1001 + i as u32,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    };
    RegionPlan::analyze(&Region::from_blocks(&[block], 0x1000).unwrap())
}

fn address(displacement: u32) -> EffectiveAddress {
    EffectiveAddress {
        base: Some(Register32::Ebx),
        displacement,
        ..Default::default()
    }
}

fn load(address: EffectiveAddress) -> Operation {
    Operation::Move {
        destination: Location32::Register(Register32::Eax),
        source: Value32::Memory(address),
    }
}

fn store(address: EffectiveAddress) -> Operation {
    Operation::Move {
        destination: Location32::Memory(address),
        source: Value32::Immediate(7),
    }
}

fn x87(operation: X87Operation, address: EffectiveAddress) -> Operation {
    Operation::X87 {
        operation,
        address: Some(address),
        waiting: false,
        opcode: 0x123,
    }
}

fn simd(address: EffectiveAddress, aligned: bool) -> Operation {
    Operation::Simd(SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Memory(address),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 16,
        immediate: None,
        aligned,
        scalar: false,
        implicit_address: None,
    })
}

#[test]
fn mixed_scalar_rmw_and_direct_numeric_accesses_share_translation_not_data_values() {
    let plan = plan(&[
        load(address(0)),
        store(address(4)),
        Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Memory(address(8)),
        },
        x87(X87Operation::Load(X87Format::Float32), address(12)),
        x87(
            X87Operation::BinaryMemory {
                kind: X87Binary::Add,
                format: X87Format::Float32,
            },
            address(16),
        ),
        x87(
            X87Operation::Store {
                format: X87Format::Float64,
                pop: true,
                truncate: false,
            },
            address(20),
        ),
        simd(address(32), true),
        simd(address(48), true),
    ]);
    assert!(plan.has_access_groups());
    let groups = plan.access_groups(BlockId(0));
    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!(
        (group.min_delta, group.max_end, group.permissions),
        (0, 64, 3)
    );
    assert_eq!(group.members.len(), 8);
    assert_eq!(group.alignment_admission(), (15, 0, true));
    assert_ne!(
        plan.frame(plan.instruction(BlockId(0), 1).before)
            .memory_effect,
        plan.frame(plan.instruction(BlockId(0), 1).after)
            .memory_effect
    );
    assert_eq!(
        group.authority, 0,
        "a data transaction does not change mapping authority"
    );
    let rmw = plan
        .access_member(BlockId(0), 2, address(8), 4, false)
        .unwrap();
    assert!(rmw.read && rmw.write);
    assert_eq!(
        plan.access_member(BlockId(0), 2, address(8), 4, true),
        Some(rmw)
    );
    assert!(
        plan.access_member(BlockId(0), 0, address(0), 4, true)
            .is_none()
    );
    assert!(
        plan.access_member(BlockId(0), 0, address(4), 4, false)
            .is_none()
    );
    assert!(
        plan.access_member(BlockId(0), 0, address(0), 2, false)
            .is_none()
    );
}

#[test]
fn changing_an_address_register_creates_a_distinct_group() {
    let plan = plan(&[
        load(address(0)),
        load(address(4)),
        Operation::Binary {
            kind: BinaryKind::Add,
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Immediate(4),
        },
        load(address(0)),
        load(address(4)),
    ]);
    let groups = plan.access_groups(BlockId(0));
    assert_eq!(groups.len(), 2);
    assert_ne!(groups[0].base, groups[1].base);
    assert_eq!((groups[0].first_prefix, groups[0].last_prefix), (0, 1));
    assert_eq!((groups[1].first_prefix, groups[1].last_prefix), (3, 4));
}

#[test]
fn canonical_control_helper_splits_authority_without_reusing_a_slot() {
    let plan = plan(&[
        load(address(0)),
        load(address(4)),
        Operation::ClearX87Exceptions,
        load(address(0)),
        load(address(4)),
    ]);
    let groups = plan.access_groups(BlockId(0));
    assert_eq!(groups.len(), 2);
    assert_ne!(groups[0].authority, groups[1].authority);
    assert_eq!((groups[0].slot, groups[1].slot), (0, 1));
}

#[test]
fn signed_modular_displacements_keep_a_bounded_envelope() {
    let plan = plan(&[load(address(u32::MAX - 3)), store(address(0))]);
    let group = &plan.access_groups(BlockId(0))[0];
    assert_eq!((group.min_delta, group.max_end), (0, 8));
    assert_eq!(group.members[1].delta, 4);
    // Actual logical/linear wrap is still a runtime admission condition.
    assert_eq!(group.anchor.displacement, u32::MAX - 3);
    let wide = self::plan(&[load(address(0)), load(address(4093))]);
    assert!(
        !wide.has_access_groups(),
        "no single-page envelope can cover these widths"
    );
}

#[test]
fn alignment_congruences_are_precomputed_and_inconsistent_groups_decline_privately() {
    let plan = plan(&[simd(address(0), true), simd(address(4), true)]);
    let group = &plan.access_groups(BlockId(0))[0];
    assert_eq!(group.alignment_mask, 15);
    assert!(!group.alignment_compatible);
    let plan = self::plan(&[load(address(0)), simd(address(4), true)]);
    assert_eq!(
        plan.access_groups(BlockId(0))[0].alignment_admission(),
        (15, 12, true)
    );
}

#[test]
fn size_segment_and_scale_are_part_of_the_group_identity_and_eight_slots_are_bounded() {
    let mut operations = Vec::new();
    for (i, segment) in [
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ]
    .into_iter()
    .enumerate()
    {
        for scale in [1, 2, 4] {
            let a = EffectiveAddress {
                index: Some(Register32::Esi),
                scale,
                segment: Some(segment),
                address_size: if i == 1 {
                    AddressSize::Bits16
                } else {
                    AddressSize::Bits32
                },
                ..address(0)
            };
            operations.extend([
                load(a),
                load(EffectiveAddress {
                    displacement: 4,
                    ..a
                }),
            ]);
        }
    }
    let plan = plan(&operations);
    let groups = plan.access_groups(BlockId(0));
    assert_eq!(groups.len(), 8);
    for (slot, group) in groups.iter().enumerate() {
        assert_eq!(usize::from(group.slot), slot);
        assert_eq!(group.members.len(), 2);
    }
    assert!(
        plan.access_member(
            BlockId(0),
            16,
            EffectiveAddress {
                index: Some(Register32::Esi),
                scale: 4,
                segment: Some(SegmentRegister::Gs),
                ..address(0)
            },
            4,
            false
        )
        .is_none()
    );
}

#[test]
fn lone_accesses_implicit_stacks_and_unsupported_numeric_widths_remain_unplanned() {
    assert!(
        !plan(&[Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Memory(address(0))
        }])
        .has_access_groups()
    );
    assert!(
        !plan(&[
            Operation::Push {
                source: Value32::Register(Register32::Eax)
            },
            Operation::Push {
                source: Value32::Register(Register32::Eax)
            }
        ])
        .has_access_groups()
    );
    assert!(
        !plan(&[
            x87(X87Operation::Load(X87Format::Float64), address(0)),
            x87(X87Operation::Load(X87Format::Float64), address(8))
        ])
        .has_access_groups()
    );
}
