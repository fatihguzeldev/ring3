use super::*;
use crate::cpu::{
    core::{
        program,
        region::{
            Region,
            values::executable::{EffectKind, ExecutableProgram},
        },
    },
    x86::{
        Register32,
        ir::{BranchTarget, EffectiveAddress, Location32, Value32},
    },
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
fn fp(operation: X87Operation) -> Operation {
    Operation::X87 {
        operation,
        address: None,
        opcode: 0xc1,
        waiting: true,
    }
}
fn memory_fp(operation: X87Operation, offset: u32) -> Operation {
    Operation::X87 {
        operation,
        address: Some(EffectiveAddress {
            base: Some(Register32::Ebx),
            displacement: offset,
            ..Default::default()
        }),
        opcode: 0x103,
        waiting: true,
    }
}
fn jump(pc: u32) -> Operation {
    Operation::Jump {
        target: BranchTarget::Direct(GuestAddress(pc)),
    }
}
fn plan(blocks: &[program::Block]) -> (RegionPlan, ExecutableProgram) {
    let region = Region::from_blocks(blocks, blocks[0].entry).unwrap();
    let values = RegionPlan::analyze(&region);
    let program = ExecutableProgram::analyze(&region, &values);
    (values, program)
}

#[test]
fn native_load_results_and_rounded_arithmetic_are_consumed_typed_effects() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            memory_fp(X87Operation::Load(X87Format::Float32), 4),
            Operation::Move {
                destination: Location32::Register(Register32::Edx),
                source: Value32::Memory(EffectiveAddress {
                    base: Some(Register32::Esi),
                    ..Default::default()
                }),
            },
            fp(X87Operation::Binary {
                kind: X87Binary::Multiply,
                destination: 0,
                source: 1,
                pop: false,
            }),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    assert_eq!(effects.len(), 4);
    let EffectKind::Fp(load) = &effects[1].kind else {
        panic!("typed FLD")
    };
    let FpEffectKind::Load32 { result: left, .. } = load.kind else {
        panic!("typed load result")
    };
    let FpPayload::Native { value, .. } = effects[2].before.fp.payload[0] else {
        panic!("native value before read")
    };
    assert_eq!(value, left);
    assert_eq!(
        effects[2].before.fp.payload, effects[2].after.fp.payload,
        "an ordered GP read preserves FP SSA values"
    );
    let EffectKind::Fp(multiply) = &effects[3].kind else {
        panic!("typed multiply")
    };
    assert!(matches!(
        multiply.kind,
        FpEffectKind::Binary {
            kind: X87Binary::Multiply,
            exact_product: true,
            ..
        }
    ));
    assert_eq!(multiply.status_before.bits, effects[2].after.fp.status.bits);
    assert!(matches!(
        program.value(left).node,
        ValueNode::Fp(FpValueNode::EffectResult { .. })
    ));
    assert_eq!(program.value(left).ty, ValueType::F64);
    assert!(multiply.definitions().contains(&multiply.status_after.bits));
    assert!(
        multiply
            .definitions()
            .contains(&multiply.status_after.pending)
    );
}

#[test]
fn popped_empty_slot_keeps_its_exact_produced_payload_as_a_recovery_root() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            fp(X87Operation::StoreRegister {
                index: 0,
                pop: true,
            }),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let payload = effects[0].after.fp.payload[0];
    assert_eq!(effects[1].after.fp.payload[7], payload);
    assert_eq!(effects[1].after.fp.top_delta, 0);
    assert_eq!(effects[1].after.fp.written, 0x80);
    let tag = effects[1].after.fp.tags[7];
    assert!(matches!(
        program.value(tag).node,
        ValueNode::Fp(FpValueNode::I32(3))
    ));
    for value in payload.roots() {
        assert!(effects[1].after.fp.roots().contains(&value));
    }
    assert_eq!(
        effects[1].before.fp.status.bits,
        effects[1].after.fp.status.bits
    );
    assert_eq!(
        effects[1].before.fp.status.pending,
        effects[1].after.fp.status.pending
    );
}

#[test]
fn discarded_arithmetic_payload_never_discards_its_sticky_status_effect() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            fp(X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 0,
                source: 0,
                pop: false,
            }),
            memory_fp(X87Operation::Load(X87Format::Float32), 4),
            fp(X87Operation::Binary {
                kind: X87Binary::Multiply,
                destination: 0,
                source: 0,
                pop: false,
            }),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let EffectKind::Fp(first) = &effects[1].kind else {
        panic!("first arithmetic")
    };
    let EffectKind::Fp(second) = &effects[3].kind else {
        panic!("second arithmetic")
    };
    assert_ne!(
        first.id, second.id,
        "original rounding points are not interned"
    );
    assert_ne!(first.status_after.bits, first.status_before.bits);
    let EffectKind::Fp(load) = &effects[2].kind else {
        panic!("intervening load")
    };
    assert_eq!(load.status_before.bits, first.status_after.bits);
    assert_eq!(second.status_before.bits, load.status_after.bits);
    assert_eq!(
        first.status_after.left,
        first.definitions()[first.definitions().len() - 3]
    );
}

#[test]
fn computation_metadata_uses_current_segment_selector_not_historical_fip_cs() {
    let (_, program) = plan(&[block(
        0x1000,
        &[memory_fp(X87Operation::Load(X87Format::Float32), 12)],
    )]);
    let effect = program.block(BlockId(0)).effects().next().unwrap();
    assert!(matches!(
        program.value(effect.after.fp.metadata.cs).node,
        ValueNode::Fp(FpValueNode::Input {
            field: FpInput::Selector(crate::cpu::x86::SegmentRegister::Cs),
            ..
        })
    ));
    assert!(matches!(
        program.value(effect.after.fp.metadata.ds).node,
        ValueNode::Fp(FpValueNode::Input {
            field: FpInput::Selector(crate::cpu::x86::SegmentRegister::Ds),
            ..
        })
    ));
}

#[test]
fn scalar_bridge_contract_preserves_incoming_changed_payload_without_importing_new_fp() {
    let (values, program) = plan(&[
        block(
            0x1000,
            &[
                memory_fp(X87Operation::Load(X87Format::Float32), 0),
                jump(0x1100),
            ],
        ),
        block(0x1100, &[Operation::Nop, jump(0x1200)]),
        block(
            0x1200,
            &[memory_fp(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false,
                },
                8,
            )],
        ),
    ]);
    let params = program.fp_parameters(BlockId(1));
    assert!(
        params.header,
        "an incoming FP view remains owned through a scalar bridge"
    );
    assert!(
        !params.imports,
        "an external scalar bridge executes no FP imports"
    );
    assert_ne!(
        params.payloads, 0,
        "the changed physical payload is an explicit continuation argument"
    );
    assert_ne!(
        params.tags, 0,
        "tag ownership is independent from payload transport"
    );
    assert_eq!(
        program.block(BlockId(1)).entry.fp.epoch,
        values
            .frame(values.block(BlockId(1)).entry)
            .fp
            .control_epoch
    );
}

#[test]
fn typed_arithmetic_preserves_inherited_dirty_payloads_and_independent_tags() {
    let (_, program) = plan(&[
        block(
            0x1000,
            &[
                memory_fp(X87Operation::Load(X87Format::Float32), 0),
                memory_fp(X87Operation::Load(X87Format::Float32), 4),
                jump(0x1100),
            ],
        ),
        block(
            0x1100,
            &[
                fp(X87Operation::Binary {
                    kind: X87Binary::Multiply,
                    destination: 0,
                    source: 1,
                    pop: false,
                }),
                jump(0x1200),
            ],
        ),
        block(
            0x1200,
            &[memory_fp(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false,
                },
                8,
            )],
        ),
    ]);
    let parameters = program.fp_parameters(BlockId(2));
    assert_eq!(
        parameters.payloads & 3,
        3,
        "overwriting ST0 does not erase the inherited ST1 physical payload"
    );
    assert_eq!(
        parameters.tags & 3,
        3,
        "the prior independent tag writes remain owned too"
    );
}

#[test]
fn popped_arithmetic_result_uses_its_own_post_pop_domain() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            memory_fp(X87Operation::Load(X87Format::Float32), 4),
            memory_fp(X87Operation::Load(X87Format::Float32), 8),
            fp(X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 1,
                source: 0,
                pop: true,
            }),
        ],
    )]);
    let effect = program.block(BlockId(0)).effects().last().unwrap();
    let FpPayload::Native {
        domain: result_domain,
        ..
    } = effect.after.fp.payload[0]
    else {
        panic!("native rounded result")
    };
    let FpPayload::Native {
        domain: neighbor_domain,
        ..
    } = effect.after.fp.payload[1]
    else {
        panic!("untouched native input")
    };
    assert_eq!(neighbor_domain, NativeDomain::Native24);
    assert_eq!(
        result_domain,
        NativeDomain::InSingle,
        "the untouched neighbor's Native24 proof does not certify FADDP"
    );
}

#[test]
fn canonical_environment_barrier_has_fresh_post_effect_inputs_and_kills_native_obligations() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            memory_fp(X87Operation::LoadEnvironment { operand16: false }, 8),
            fp(X87Operation::ChangeSign),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    assert!(matches!(effects[1].kind, EffectKind::Exact(_)));
    assert_ne!(effects[1].before.fp.epoch, effects[1].after.fp.epoch);
    assert!(effects[1].after.fp.ingress.is_empty());
    assert!(matches!(
        effects[1].after.fp.payload[0],
        FpPayload::Tagged { .. }
    ));
    assert_eq!(effects[1].after.fp.written, 0);
    assert!(matches!(
        effects[2].kind,
        EffectKind::Fp(FpEffect {
            kind: FpEffectKind::Sign { .. },
            ..
        })
    ));
}

#[test]
fn observed_call_ret_cycle_has_explicit_fp_arguments_through_scalar_blocks() {
    use crate::cpu::x86::ir::Condition;
    fn instruction(pc: u32, next: u32, operation: Operation) -> program::Instruction {
        program::Instruction {
            pc,
            next,
            operation: Some(operation),
            gate: None,
        }
    }
    let blocks = [
        program::Block {
            entry: 0x1000,
            instructions: vec![instruction(
                0x1000,
                0x1005,
                Operation::Call {
                    target: BranchTarget::Direct(GuestAddress(0x1100)),
                },
            )],
        },
        program::Block {
            entry: 0x1005,
            instructions: vec![
                instruction(
                    0x1005,
                    0x1008,
                    Operation::Binary {
                        kind: crate::cpu::x86::ir::BinaryKind::Add,
                        destination: Location32::Register(Register32::Eax),
                        source: Value32::Immediate(1),
                    },
                ),
                instruction(
                    0x1008,
                    0x1009,
                    Operation::Unary {
                        kind: crate::cpu::x86::ir::UnaryKind::Dec,
                        destination: Location32::Register(Register32::Ecx),
                    },
                ),
                instruction(
                    0x1009,
                    0x100b,
                    Operation::ConditionalJump {
                        condition: Condition::NotEqual,
                        target: GuestAddress(0x1000),
                    },
                ),
            ],
        },
        program::Block {
            entry: 0x100b,
            instructions: vec![instruction(0x100b, 0x100c, Operation::Nop)],
        },
        program::Block {
            entry: 0x1100,
            instructions: vec![
                instruction(
                    0x1100,
                    0x1102,
                    fp(X87Operation::Binary {
                        kind: X87Binary::Add,
                        destination: 0,
                        source: 1,
                        pop: false,
                    }),
                ),
                instruction(0x1102, 0x1103, Operation::Return { stack_adjust: 0 }),
            ],
        },
    ];
    let edges = [
        (0x1000, 0x1100),
        (0x1005, 0x1008),
        (0x1008, 0x1009),
        (0x1009, 0x1000),
        (0x1009, 0x100b),
        (0x1100, 0x1102),
        (0x1102, 0x1005),
    ];
    let region = Region::from_observed(&blocks, 0x1000, &edges).unwrap();
    let values = RegionPlan::analyze(&region);
    let program = ExecutableProgram::analyze(&region, &values);
    for (index, block) in region.blocks.iter().enumerate() {
        let params = program.fp_parameters(BlockId(index));
        assert!(
            params.header,
            "the active FP view crosses block {:x}",
            block.entry
        );
        let required = if block.entry == 0x100b { 1 } else { 3 };
        assert_eq!(
            params.payloads & required,
            required,
            "both changed ST0 and downstream clean ST1 cross block {:x}",
            block.entry
        );
        assert_eq!(
            params.tags & required,
            required,
            "their independent occupied tags cross block {:x}",
            block.entry
        );
        assert_eq!(
            params.imports,
            block.entry == 0x1100,
            "only the actual FP block imports public FP inputs"
        );
    }
}

#[test]
fn downstream_fp_demands_follow_pop_aliases_through_scalar_predecessors() {
    let (_, program) = plan(&[
        block(0x1000, &[Operation::Nop, jump(0x1100)]),
        block(
            0x1100,
            &[
                fp(X87Operation::StoreRegister {
                    index: 0,
                    pop: true,
                }),
                jump(0x1200),
            ],
        ),
        block(
            0x1200,
            &[fp(X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 0,
                source: 1,
                pop: false,
            })],
        ),
    ]);
    for id in [BlockId(0), BlockId(1)] {
        let params = program.fp_parameters(id);
        assert_eq!(
            params.payloads & 7,
            7,
            "post-pop ST0/ST1 are pre-pop ST1/ST2, independently of the popped ghost"
        );
        assert_eq!(params.tags & 7, 7);
    }
}

#[test]
fn canonical_fp_barrier_stops_backward_old_payload_demands() {
    let (_, program) = plan(&[
        block(0x1000, &[Operation::Nop, jump(0x1100)]),
        block(
            0x1100,
            &[
                memory_fp(X87Operation::LoadEnvironment { operand16: false }, 12),
                jump(0x1200),
            ],
        ),
        block(
            0x1200,
            &[fp(X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 0,
                source: 1,
                pop: false,
            })],
        ),
    ]);
    assert_eq!(
        program.fp_parameters(BlockId(0)).payloads,
        0,
        "a post-canonical actual input is not an old native argument"
    );
    assert_eq!(program.fp_parameters(BlockId(1)).payloads, 0);
    assert_eq!(program.fp_parameters(BlockId(2)).payloads & 3, 3);
}

#[test]
fn exact_status_observer_captures_resolved_status_without_losing_payloads() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            memory_fp(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Add,
                    format: X87Format::Float32,
                },
                4,
            ),
            Operation::X87StatusToAx,
        ],
    )]);
    let effect = program.block(BlockId(0)).effects().last().unwrap();
    assert!(matches!(effect.kind, EffectKind::Exact(_)));
    assert_eq!(effect.contract.fp, FpEffectClass::Observe);
    assert_eq!(effect.before.fp.payload, effect.after.fp.payload);
    assert_eq!(effect.before.fp.tags, effect.after.fp.tags);
    assert_eq!(effect.before.fp.control, effect.after.fp.control);
    assert_eq!(effect.before.fp.top, effect.after.fp.top);
    assert_eq!(effect.before.fp.epoch, effect.after.fp.epoch);
    for id in effect.after.fp.status.roots() {
        assert!(!effect.before.fp.status.roots().contains(&id));
        assert!(effect.definitions().contains(&id));
        assert!(
            matches!(program.value(id).node,ValueNode::Fp(FpValueNode::EffectResult {effect:id_effect,..}) if id_effect==effect.id)
        );
    }
}

#[test]
fn inline_wait_observes_status_and_preserves_unpublished_fp_values() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            fp(X87Operation::Wait),
        ],
    )]);
    let effect = program.block(BlockId(0)).effects().last().unwrap();
    assert_eq!(effect.contract.fp, FpEffectClass::Observe);
    assert_eq!(effect.before.fp.payload, effect.after.fp.payload);
    assert_eq!(effect.before.fp.written, effect.after.fp.written);
    assert_eq!(effect.before.fp.tag_written, effect.after.fp.tag_written);
    assert_ne!(effect.before.fp.status.bits, effect.after.fp.status.bits);
}

#[test]
fn tagged_input_certificate_is_a_complete_typed_entry_argument() {
    let (_, program) = plan(&[block(
        0x1000,
        &[memory_fp(
            X87Operation::BinaryMemory {
                kind: X87Binary::Add,
                format: X87Format::Float32,
            },
            4,
        )],
    )]);
    let body = program.block(BlockId(0));
    let entry = &body.entry.fp;
    let FpPayload::Tagged {
        valid, certificate, ..
    } = entry.payload[0]
    else {
        panic!("an arbitrary external input remains tagged")
    };
    assert_eq!(program.value(certificate).ty, ValueType::I32);
    assert!(matches!(
        program.value(certificate).node,
        ValueNode::Fp(FpValueNode::InputCertificate {
            point: ProgramPoint { block: BlockId(0), prefix: 0 },
            logical: 0,
            control,
            valid: actual_valid,
        }) if control == entry.control && actual_valid == valid
    ));
    assert_eq!(
        program.dependency_order(&[certificate]),
        vec![entry.control, valid, certificate],
        "the certificate is keyed by the same actual validity and CW values"
    );
    assert_eq!(entry.payload[0].roots().len(), 5);
    assert!(
        program
            .fp_parameters(BlockId(0))
            .roots(entry)
            .contains(&certificate),
        "the public adapter snapshots the certificate before any effect"
    );
    assert!(
        body.effects()
            .next()
            .unwrap()
            .inputs()
            .contains(&certificate)
    );
}

#[test]
fn tagged_certificate_follows_aliases_but_not_changed_or_barrier_payloads() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            fp(X87Operation::LoadRegister { index: 0 }),
            Operation::X87StatusToAx,
            fp(X87Operation::ChangeSign),
            memory_fp(X87Operation::LoadEnvironment { operand16: false }, 12),
        ],
    )]);
    let body = program.block(BlockId(0));
    let effects: Vec<_> = body.effects().collect();
    let original = body.entry.fp.payload[0];
    let FpPayload::Tagged {
        certificate: original_certificate,
        ..
    } = original
    else {
        panic!("tagged input")
    };
    assert_eq!(effects[0].after.fp.payload[0], original);
    assert_eq!(effects[0].after.fp.payload[1], original);
    assert_eq!(effects[1].after.fp.payload[0], original);
    let FpPayload::Tagged {
        certificate: changed_certificate,
        ..
    } = effects[2].after.fp.payload[0]
    else {
        panic!("changed opaque payload")
    };
    assert!(matches!(
        program.value(changed_certificate).node,
        ValueNode::Fp(FpValueNode::I32(0))
    ));
    assert_eq!(
        effects[2].after.fp.payload[1], original,
        "changing one alias never changes the original value's certificate"
    );
    let after = &effects[3].after.fp;
    let FpPayload::Tagged {
        valid, certificate, ..
    } = after.payload[0]
    else {
        panic!("fresh canonical payload")
    };
    assert_ne!(certificate, original_certificate);
    assert_ne!(after.control, body.entry.fp.control);
    assert!(matches!(
        program.value(certificate).node,
        ValueNode::Fp(FpValueNode::InputCertificate {
            point: ProgramPoint { prefix: 4, .. },
            control,
            valid: actual_valid,
            ..
        }) if control == after.control && actual_valid == valid
    ));
}

#[test]
fn downstream_tagged_certificate_is_carried_through_scalar_predecessors() {
    let (_, program) = plan(&[
        block(0x1000, &[Operation::Nop, jump(0x1100)]),
        block(0x1100, &[Operation::Nop, jump(0x1200)]),
        block(
            0x1200,
            &[memory_fp(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Add,
                    format: X87Format::Float32,
                },
                4,
            )],
        ),
    ]);
    for index in 0..3 {
        let entry = &program.block(BlockId(index)).entry.fp;
        let FpPayload::Tagged { certificate, .. } = entry.payload[0] else {
            panic!("unknown incoming payload")
        };
        let required = program.fp_parameters(BlockId(index)).roots(entry);
        for id in entry.payload[0].roots() {
            assert!(required.contains(&id));
        }
        assert!(required.contains(&certificate));
    }
}

#[test]
fn opaque_scalar_definitions_preserve_unpublished_fp_values_and_effects() {
    use crate::cpu::x86::ir::{ShiftCount, ShiftKind};
    for operation in [
        Operation::Shift {
            kind: ShiftKind::Shl,
            destination: Location32::Register(Register32::Eax),
            count: ShiftCount::Immediate(1),
        },
        Operation::Shift {
            kind: ShiftKind::Shr,
            destination: Location32::Register(Register32::Eax),
            count: ShiftCount::Immediate(1),
        },
        Operation::SignedMultiply {
            destination: Register32::Eax,
            source: Location32::Register(Register32::Ecx),
            immediate: None,
        },
        Operation::ByteSwap {
            destination: Register32::Eax,
        },
        Operation::Shift {
            kind: ShiftKind::Shl,
            destination: Location32::Memory(EffectiveAddress {
                base: Some(Register32::Esi),
                ..Default::default()
            }),
            count: ShiftCount::Immediate(1),
        },
    ] {
        let (_, program) = plan(&[block(
            0x1000,
            &[
                memory_fp(X87Operation::Load(X87Format::Float32), 0),
                operation,
                memory_fp(
                    X87Operation::Store {
                        format: X87Format::Float32,
                        pop: true,
                        truncate: false,
                    },
                    4,
                ),
            ],
        )]);
        let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
        let scalar = effects[1];
        assert!(matches!(scalar.kind, EffectKind::Exact(_)));
        assert_eq!(scalar.contract.fp, FpEffectClass::Preserve, "{operation:?}");
        assert_eq!(scalar.contract.gpr_reads, u8::MAX);
        assert_eq!(scalar.contract.gpr_writes, u8::MAX);
        assert_eq!(scalar.before.fp.payload, scalar.after.fp.payload);
        assert_eq!(scalar.before.fp.tags, scalar.after.fp.tags);
        assert_eq!(scalar.before.fp.top, scalar.after.fp.top);
        assert_eq!(scalar.before.fp.control, scalar.after.fp.control);
        assert_eq!(scalar.before.fp.epoch, scalar.after.fp.epoch);
        assert_eq!(
            scalar.before.fp.status.roots(),
            scalar.after.fp.status.roots()
        );
        assert_eq!(scalar.before.fp.written, scalar.after.fp.written);
        assert_eq!(scalar.before.fp.tag_written, scalar.after.fp.tag_written);
        let EffectKind::Fp(store) = &effects[2].kind else {
            panic!("typed store after the scalar exact effect")
        };
        assert!(matches!(store.kind,
            FpEffectKind::Store { source, .. } if source == scalar.before.fp.payload[0]));
    }
}

#[test]
fn typed_control_write_keeps_dirty_payload_tag_top_and_metadata_but_rebases_cw_proofs() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            memory_fp(X87Operation::LoadControl, 8),
            Operation::X87ControlToMemory {
                address: EffectiveAddress {
                    base: Some(Register32::Ebx),
                    displacement: 12,
                    ..Default::default()
                },
            },
            fp(X87Operation::ChangeSign),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let cw = &effects[1];
    assert!(matches!(
        cw.kind,
        EffectKind::Fp(FpEffect {
            kind: FpEffectKind::LoadControl { .. },
            waiting: true,
            ..
        })
    ));
    let before = &cw.before.fp;
    let after = &cw.after.fp;
    assert_ne!(before.control, after.control);
    assert_ne!(before.epoch, after.epoch);
    assert_ne!(before.status.bits, after.status.bits);
    assert_ne!(before.status.pending, after.status.pending);
    assert_eq!(before.tags, after.tags);
    assert_eq!(before.top, after.top);
    assert_eq!(before.top_delta, after.top_delta);
    assert_eq!(before.written, after.written);
    assert_eq!(before.tag_written, after.tag_written);
    for (old, new) in before.payload.into_iter().zip(after.payload) {
        match (old, new) {
            (
                FpPayload::Native { value: old, .. },
                FpPayload::Native {
                    value: new,
                    domain: NativeDomain::Exact,
                    epoch,
                },
            ) => {
                assert_eq!(old, new);
                assert_eq!(epoch, after.epoch);
            }
            (
                FpPayload::Tagged {
                    low: l,
                    high: h,
                    native: n,
                    valid: v,
                    ..
                },
                FpPayload::Tagged {
                    low,
                    high,
                    native,
                    valid,
                    certificate,
                },
            ) => {
                assert_eq!((l, h, n, v), (low, high, native, valid));
                assert!(matches!(
                    program.value(certificate).node,
                    ValueNode::Fp(FpValueNode::I32(0))
                ));
            }
            pair => panic!("CW write changed an architectural payload: {pair:?}"),
        }
    }
    assert_eq!(
        (
            before.metadata.fip,
            before.metadata.fdp,
            before.metadata.opcode,
            before.metadata.cs,
            before.metadata.ds
        ),
        (
            after.metadata.fip,
            after.metadata.fdp,
            after.metadata.opcode,
            after.metadata.cs,
            after.metadata.ds
        )
    );
    let store = &effects[2];
    assert!(matches!(
        store.kind,
        EffectKind::Fp(FpEffect {
            kind: FpEffectKind::StoreControl { .. },
            waiting: false,
            ..
        })
    ));
    assert_eq!(store.before.fp.payload, store.after.fp.payload);
    assert_eq!(store.before.fp.control, store.after.fp.control);
    assert_eq!(store.before.fp.status.bits, store.after.fp.status.bits);
    assert_eq!(
        store.before.fp.status.pending,
        store.after.fp.status.pending
    );
    assert_eq!(store.before.fp.metadata.fip, store.after.fp.metadata.fip);
}

#[test]
fn integer_load64_defines_all_exact_payload_lanes_and_pop_preserves_the_empty_ghost() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Integer64), 0),
            memory_fp(
                X87Operation::Store {
                    format: X87Format::Integer64,
                    truncate: true,
                    pop: true,
                },
                8,
            ),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let load = &effects[0];
    let EffectKind::Fp(effect) = &load.kind else {
        panic!("typed FILD64")
    };
    let FpEffectKind::IntegerLoad {
        result: payload,
        format: X87Format::Integer64,
        ..
    } = effect.kind
    else {
        panic!("integer64 load")
    };
    assert!(matches!(payload, FpPayload::Tagged { .. }));
    assert_eq!(payload.roots().len(), 5);
    for value in payload.roots() {
        assert!(effect.definitions().contains(&value));
    }
    assert_eq!(load.after.fp.top_delta, 7);
    assert_eq!(effects[1].after.fp.top_delta, 0);
    assert_eq!(effects[1].after.fp.payload[7], payload);
    assert!(matches!(
        program.value(effects[1].after.fp.tags[7]).node,
        ValueNode::Fp(FpValueNode::I32(3))
    ));
}

#[test]
fn incoming_payload_dirtiness_tracks_top_rotation_control_and_canonical_reset() {
    let (_, program) = plan(&[
        block(
            0x1000,
            &[
                memory_fp(X87Operation::Load(X87Format::Float32), 0),
                jump(0x1100),
            ],
        ),
        block(0x1100, &[Operation::Nop, jump(0x1200)]),
        block(
            0x1200,
            &[
                fp(X87Operation::StoreRegister {
                    index: 0,
                    pop: true,
                }),
                jump(0x1300),
            ],
        ),
        block(
            0x1300,
            &[memory_fp(X87Operation::LoadControl, 4), jump(0x1400)],
        ),
        block(
            0x1400,
            &[
                memory_fp(X87Operation::LoadEnvironment { operand16: false }, 8),
                jump(0x1500),
            ],
        ),
        block(0x1500, &[Operation::Nop]),
    ]);
    let masks: Vec<_> = (0..6)
        .map(|index| program.fp_parameters(BlockId(index)).incoming_dirty)
        .collect();
    assert_eq!(
        masks,
        [0, 1, 1, 0x80, 0x80, 0],
        "fresh entry is clean; a pop carries the changed empty ghost; FLDCW preserves it; FLDENV replaces it"
    );
    assert_eq!(program.block(BlockId(2)).exit.fp.top_delta, 1);
    assert_eq!(program.block(BlockId(2)).exit.fp.written, 0x80);
    assert_ne!(
        program.block(BlockId(3)).entry.fp.epoch,
        program.block(BlockId(3)).exit.fp.epoch,
        "a real CW epoch change does not discard the incoming payload write"
    );
}

#[test]
fn incoming_payload_dirtiness_includes_the_backedge_empty_ghost_at_a_public_root() {
    let (_, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float32), 0),
            fp(X87Operation::StoreRegister {
                index: 0,
                pop: true,
            }),
            jump(0x1000),
        ],
    )]);
    let body = program.block(BlockId(0));
    assert_eq!(body.exit.fp.top_delta, 0);
    assert_eq!(body.exit.fp.written, 0x80);
    let parameters = program.fp_parameters(BlockId(0));
    assert_eq!(
        parameters.incoming_dirty, 0x80,
        "a public root can also be entered by a real dirty internal backedge"
    );
    assert_eq!(parameters.payloads & 0x80, 0x80);
    assert_eq!(parameters.tags & 0x80, 0x80);
}

#[test]
fn binary64_load_defines_only_exact_native_payload_and_keeps_control_epoch() {
    let (values, program) = plan(&[block(
        0x1000,
        &[
            memory_fp(X87Operation::Load(X87Format::Float64), 0),
            memory_fp(
                X87Operation::Store {
                    format: X87Format::Float64,
                    pop: true,
                    truncate: false,
                },
                8,
            ),
        ],
    )]);
    let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
    let load = &effects[0];
    let EffectKind::Fp(effect) = &load.kind else {
        panic!("typed binary64 load")
    };
    let FpEffectKind::Load64 { result, .. } = effect.kind else {
        panic!("binary64 result")
    };
    assert!(
        matches!(load.after.fp.payload[0], FpPayload::Native { value, domain: NativeDomain::Exact, epoch }
        if value == result && epoch == load.before.fp.epoch)
    );
    assert_eq!(load.before.fp.control, load.after.fp.control);
    assert_eq!(load.before.fp.epoch, load.after.fp.epoch);
    let value = values.frame(load.after.frame).fp.payload[0].values()[0];
    assert!(!values.proofs.iter().any(|proof| proof.value == Some(value)
        && matches!(
            proof.kind,
            ProofKind::BoundedF64 | ProofKind::Native24 | ProofKind::Native24InSingle
        )));
    assert_eq!(effects[1].after.fp.payload[7], load.after.fp.payload[0]);
    assert!(matches!(
        program.value(effects[1].after.fp.tags[7]).node,
        ValueNode::Fp(FpValueNode::I32(3))
    ));
}

#[test]
fn typed_comparison_pop_changes_only_status_tags_top_and_metadata() {
    for pop in 0..=2 {
        let (_, program) = plan(&[block(
            0x1000,
            &[
                memory_fp(X87Operation::Load(X87Format::Float64), 0),
                memory_fp(X87Operation::Load(X87Format::Float64), 8),
                fp(X87Operation::Compare {
                    index: 1,
                    quiet: true,
                    pop,
                    integer_flags: false,
                }),
            ],
        )]);
        let effects: Vec<_> = program.block(BlockId(0)).effects().collect();
        let compare = &effects[2];
        let EffectKind::Fp(effect) = &compare.kind else {
            panic!("typed comparison")
        };
        assert!(matches!(
            effect.kind,
            FpEffectKind::Compare {
                right: FpOperand::Payload(_),
                quiet: true,
                ..
            }
        ));
        assert!(matches!(effect.status_effect, StatusEffect::Compare));
        assert_eq!(compare.before.gpr, compare.after.gpr);
        assert_eq!(compare.before.flags, compare.after.flags);
        assert_eq!(compare.before.fp.control, compare.after.fp.control);
        assert_eq!(compare.before.fp.epoch, compare.after.fp.epoch);
        let mut payloads = compare.before.fp.payload;
        payloads.rotate_left(pop as usize);
        assert_eq!(compare.after.fp.payload, payloads);
        assert_eq!(
            compare.after.fp.top_delta,
            (compare.before.fp.top_delta + i8::try_from(pop).unwrap()).rem_euclid(8)
        );
        assert_ne!(compare.before.fp.status.bits, compare.after.fp.status.bits);
        assert_ne!(
            compare.before.fp.status.pending,
            compare.after.fp.status.pending
        );
        assert_eq!(compare.before.fp.status.left, compare.after.fp.status.left);
        assert_eq!(
            compare.before.fp.status.right,
            compare.after.fp.status.right
        );
        assert_eq!(
            compare.before.fp.status.result,
            compare.after.fp.status.result
        );
        assert_eq!(
            effect.definitions(),
            vec![
                compare.after.fp.status.bits,
                compare.after.fp.status.pending
            ]
        );
        for logical in 8 - usize::from(pop)..8 {
            assert!(matches!(
                program.value(compare.after.fp.tags[logical]).node,
                ValueNode::Fp(FpValueNode::I32(3))
            ));
        }
    }
}

#[test]
fn typed_memory_comparisons_keep_exact_address_and_flags_comparisons_stay_canonical() {
    for format in [X87Format::Float32, X87Format::Float64] {
        let (_, program) = plan(&[block(
            0x1000,
            &[memory_fp(
                X87Operation::CompareMemory { format, pop: true },
                0x44,
            )],
        )]);
        let effect = program.block(BlockId(0)).effects().next().unwrap();
        let EffectKind::Fp(compare) = &effect.kind else {
            panic!("typed memory comparison")
        };
        let FpEffectKind::Compare {
            right,
            quiet: false,
            ..
        } = &compare.kind
        else {
            panic!("memory comparison operand")
        };
        let address = match (format, right) {
            (X87Format::Float32, FpOperand::Memory32(address))
            | (X87Format::Float64, FpOperand::Memory64(address)) => address,
            _ => panic!("exact memory width"),
        };
        assert!(compare.inputs().contains(&address.offset));
        assert_eq!(effect.after.fp.metadata.fdp, address.offset);
        assert_eq!(effect.after.fp.top_delta, 1);
    }
    let (_, program) = plan(&[block(
        0x1000,
        &[fp(X87Operation::Compare {
            index: 1,
            quiet: false,
            pop: 0,
            integer_flags: true,
        })],
    )]);
    let effect = program.block(BlockId(0)).effects().next().unwrap();
    assert!(matches!(effect.kind, EffectKind::Exact(_)));
    assert!(matches!(effect.contract.fp, FpEffectClass::Barrier));
}
