use super::*;
use crate::cpu::{
    core::{flags, program},
    x86::{
        Register32, SegmentRegister,
        ir::{ByteRegister, ByteValue, CarryKind, UnaryKind},
    },
};

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

fn plan(operations: &[Operation]) -> (RegionPlan, ExecutablePlan) {
    let region = Region::from_blocks(&[block(0x1000, operations)], 0x1000).unwrap();
    let values = RegionPlan::analyze(&region);
    let executable = ExecutablePlan::analyze(&region, &values);
    (values, executable)
}

fn mov(destination: Register32, source: Value32) -> Operation {
    Operation::Move {
        destination: Location32::Register(destination),
        source,
    }
}

fn binary(kind: BinaryKind, destination: Register32, source: Value32) -> Operation {
    Operation::Binary {
        kind,
        destination: Location32::Register(destination),
        source,
    }
}

fn address() -> EffectiveAddress {
    EffectiveAddress {
        base: Some(Register32::Ebx),
        ..Default::default()
    }
}

fn runs(plan: &ExecutablePlan) -> Vec<&PureRun> {
    plan.block(BlockId(0))
        .steps
        .iter()
        .filter_map(|step| match step {
            ExecutableStep::Pure(run) => Some(run),
            ExecutableStep::Effect(_) => None,
        })
        .collect()
}

fn eval(plan: &ExecutablePlan, id: ExprId, input: &[u32; 8]) -> u32 {
    match *plan.expression(id) {
        Expr32::Input { register, .. } => input[register as usize],
        Expr32::Constant(value) => value,
        Expr32::EffectResult { .. } => panic!("effect output required"),
        Expr32::Binary { kind, left, right } => {
            let left = eval(plan, left, input);
            let right = eval(plan, right, input);
            match kind {
                Binary32::Add => left.wrapping_add(right),
                Binary32::Sub => left.wrapping_sub(right),
                Binary32::And => left & right,
                Binary32::Or => left | right,
                Binary32::Xor => left ^ right,
            }
        }
        Expr32::ShiftLeft { value, amount } => {
            eval(plan, value, input).wrapping_shl(u32::from(amount))
        }
    }
}

fn recover(plan: &ExecutablePlan, map: &Recovery, input: &[u32; 8]) -> [u32; 8] {
    map.gpr.map(|value| eval(plan, value, input))
}

// This oracle uses widened integer carry/borrow and signed range, independently
// of the runtime's lazy-tuple materializer and emitted bit equations.
fn oracle_flags(base: u32, kind: BinaryKind, left: u32, right: u32, result: u32) -> u32 {
    let logical = matches!(
        kind,
        BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test
    );
    let subtraction = matches!(kind, BinaryKind::Sub | BinaryKind::Cmp);
    let wide = if subtraction {
        i64::from(left as i32) - i64::from(right as i32)
    } else {
        i64::from(left as i32) + i64::from(right as i32)
    };
    let carry = !logical
        && if subtraction {
            left < right
        } else {
            u64::from(left) + u64::from(right) > u64::from(u32::MAX)
        };
    let overflow = !logical && (wide < i64::from(i32::MIN) || wide > i64::from(i32::MAX));
    let auxiliary = !logical
        && if subtraction {
            (left & 15) < (right & 15)
        } else {
            (left & 15) + (right & 15) > 15
        };
    let bits = u32::from(carry)
        | (u32::from((result as u8).count_ones() % 2 == 0) << 2)
        | (u32::from(auxiliary) << 4)
        | (u32::from(result == 0) << 6)
        | (u32::from((result as i32) < 0) << 7)
        | (u32::from(overflow) << 11);
    ((base & !flags::ARITHMETIC) | bits) | 2
}

#[test]
fn dead_nonconstant_chains_and_duplicate_expressions_have_only_live_value_roots() {
    let operations = [
        binary(
            BinaryKind::Add,
            Register32::Eax,
            Value32::Register(Register32::Ecx),
        ),
        mov(Register32::Eax, Value32::Register(Register32::Edx)),
        Operation::Lea {
            destination: Register32::Ebx,
            address: EffectiveAddress {
                base: Some(Register32::Ecx),
                index: Some(Register32::Edx),
                scale: 2,
                displacement: 7,
                ..Default::default()
            },
        },
        Operation::Lea {
            destination: Register32::Esi,
            address: EffectiveAddress {
                base: Some(Register32::Ecx),
                index: Some(Register32::Edx),
                scale: 2,
                displacement: 7,
                ..Default::default()
            },
        },
        binary(
            BinaryKind::Xor,
            Register32::Edi,
            Value32::Register(Register32::Edi),
        ),
    ];
    let (_, executable) = plan(&operations);
    let run = runs(&executable)[0];
    assert_eq!(
        run.after.gpr[3], run.after.gpr[6],
        "equal LEAs share one expression"
    );
    let live = executable.live_expressions(run);
    assert!(
        !live
            .iter()
            .any(|&id| matches!(executable.expression(id), Expr32::Binary {
        kind: Binary32::Add, left, right,
    } if matches!(executable.expression(*left), Expr32::Input { register: 0, .. })
        && matches!(executable.expression(*right), Expr32::Input { register: 1, .. })))
    );
    let input = [9, 13, 17, 0, 0x4800, 0, 0, 99];
    assert_eq!(
        recover(&executable, &run.after, &input),
        [17, 13, 17, 54, 0x4800, 0, 54, 0]
    );
    assert_eq!(run.end_prefix, 5);
    for &id in live {
        match executable.expression(id) {
            Expr32::Binary { left, right, .. } => assert!(left.0 < id.0 && right.0 < id.0),
            Expr32::ShiftLeft { value, .. } => assert!(value.0 < id.0),
            _ => {}
        }
    }
}

#[test]
fn long_wrapping_add_chain_keeps_exact_last_flags_without_emitting_all_intermediates() {
    let operations = vec![binary(BinaryKind::Add, Register32::Eax, Value32::Immediate(1)); 128];
    let (values, executable) = plan(&operations);
    let run = runs(&executable)[0];
    let live_binary = executable
        .live_expressions(run)
        .iter()
        .filter(|&&id| matches!(executable.expression(id), Expr32::Binary { .. }))
        .count();
    assert_eq!(
        live_binary, 2,
        "only final result and last flag operand are live"
    );
    assert_eq!(run.writes, 1);
    assert_eq!(run.after.frame, values.block(BlockId(0)).exit);
    for initial in [0, u32::MAX, 0x7fff_ff80, 0xffff_ff80] {
        let input = [initial, 0, 0, 0, 0, 0, 0, 0];
        for prefix in 0..=128 {
            assert_eq!(
                recover(
                    &executable,
                    executable.block(BlockId(0)).recovery(prefix),
                    &input
                )[0],
                initial.wrapping_add(prefix)
            );
        }
        let FlagsRecipe::Arithmetic {
            kind,
            left,
            right,
            result,
            width,
        } = run.after.flags.recipe
        else {
            panic!("last flags");
        };
        let (left, right, result) = (
            eval(&executable, left, &input),
            eval(&executable, right, &input),
            eval(&executable, result, &input),
        );
        assert_eq!(
            flags::materialize(
                0x403,
                kind,
                width,
                left,
                right,
                result,
                0,
                flags::ARITHMETIC
            ),
            oracle_flags(
                0x403,
                BinaryKind::Add,
                initial.wrapping_add(127),
                1,
                initial.wrapping_add(128)
            )
        );
    }
}

#[test]
fn every_unused_memory_read_is_an_ordered_faulting_effect_and_recovery_precedes_it() {
    let load = mov(Register32::Eax, Value32::Memory(address()));
    let operations = [
        binary(BinaryKind::Add, Register32::Ecx, Value32::Immediate(7)),
        load,
        mov(Register32::Eax, Value32::Immediate(0)),
        load,
        mov(Register32::Eax, Value32::Immediate(0)),
    ];
    let (values, executable) = plan(&operations);
    let effects: Vec<_> = executable
        .block(BlockId(0))
        .steps
        .iter()
        .filter_map(|step| {
            if let ExecutableStep::Effect(effect) = step {
                Some(effect)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        effects.iter().map(|e| e.point.prefix).collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(effects.iter().all(|effect| effect.operation == Some(load)));
    assert_eq!(
        effects[0].before.frame,
        values.instruction(BlockId(0), 1).before
    );
    assert_eq!(effects[0].before.prefix, 1);
    assert_eq!(
        recover(
            &executable,
            &effects[0].before,
            &[3, 5, 0, 0x3000, 0, 0, 0, 0]
        )[1],
        12
    );
    assert!(matches!(
        executable.expression(effects[0].after.gpr[0]),
        Expr32::EffectResult {
            effect: ExecEffectId(0),
            register: 0
        }
    ));
    assert!(matches!(
        executable.expression(effects[1].after.gpr[0]),
        Expr32::EffectResult {
            effect: ExecEffectId(1),
            register: 0
        }
    ));
    assert_ne!(effects[0].after.gpr[0], effects[1].after.gpr[0]);
}

#[test]
fn partial_register_carry_direction_and_helpers_remain_exact_barriers() {
    let operations = [
        mov(Register32::Eax, Value32::Immediate(0xffff_ffff)),
        Operation::MoveByte {
            destination: ByteRegister::Ah,
            source: ByteValue::Immediate(9),
        },
        binary(BinaryKind::Adc, Register32::Eax, Value32::Immediate(1)),
        Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Register(Register32::Eax),
        },
        Operation::Direction { set: true },
        Operation::CpuId,
    ];
    let (_, executable) = plan(&operations);
    let steps = &executable.block(BlockId(0)).steps;
    assert!(matches!(steps[0], ExecutableStep::Pure(_)));
    assert_eq!(
        steps
            .iter()
            .filter(|s| matches!(s, ExecutableStep::Effect(_)))
            .count(),
        5
    );
    let effects: Vec<_> = executable.block(BlockId(0)).effects().collect();
    assert_eq!(effects[0].contract.gpr_writes, 1);
    assert_eq!(effects[1].contract.gpr_writes, 1);
    assert_eq!(effects[2].contract.gpr_writes, 1);
    assert_eq!(effects[3].contract.gpr_writes, 0);
    assert_eq!(effects[4].contract.gpr_writes, 0x0f);
    assert_eq!(
        effects[3].after.flags.origin,
        FlagsOrigin::Effect(effects[3].id)
    );
    assert_eq!(effects[4].after.flags, effects[4].before.flags);
    assert_eq!(effects[4].after.gpr[4..], effects[4].before.gpr[4..]);
}

#[test]
fn parallel_register_writes_and_lea_ignore_segment_but_keep_address16_wrap() {
    let operations = [
        mov(Register32::Edx, Value32::Register(Register32::Eax)),
        mov(Register32::Eax, Value32::Register(Register32::Ecx)),
        mov(Register32::Ecx, Value32::Register(Register32::Edx)),
        Operation::Lea {
            destination: Register32::Ebx,
            address: EffectiveAddress {
                base: Some(Register32::Eax),
                index: Some(Register32::Ecx),
                scale: 8,
                displacement: u32::MAX,
                segment: Some(SegmentRegister::Gs),
                address_size: AddressSize::Bits16,
            },
        },
    ];
    let (_, executable) = plan(&operations);
    let run = runs(&executable)[0];
    assert_eq!(
        recover(
            &executable,
            &run.after,
            &[0xffff_ffff, 0x8000_0001, 5, 0, 0, 0, 0, 0]
        ),
        [0x8000_0001, 0xffff_ffff, 0xffff_ffff, 0xfff8, 0, 0, 0, 0]
    );
}

#[test]
fn arithmetic_recipe_boundary_vectors_preserve_df_and_match_independent_flags() {
    for kind in [
        BinaryKind::Add,
        BinaryKind::Sub,
        BinaryKind::Cmp,
        BinaryKind::And,
        BinaryKind::Or,
        BinaryKind::Xor,
        BinaryKind::Test,
    ] {
        let (_, executable) = plan(&[binary(
            kind,
            Register32::Eax,
            Value32::Register(Register32::Ecx),
        )]);
        let run = runs(&executable)[0];
        for (a, b) in [
            (0, 0),
            (u32::MAX, 1),
            (0x7fff_ffff, 1),
            (0x8000_0000, 1),
            (15, 1),
            (0x80, 0x7f),
        ] {
            let input = [a, b, 0, 0, 0, 0, 0, 0];
            let FlagsRecipe::Arithmetic {
                kind: recipe,
                width,
                left,
                right,
                result,
            } = run.after.flags.recipe
            else {
                panic!("arithmetic recipe");
            };
            let actual = flags::materialize(
                0x403,
                recipe,
                width,
                eval(&executable, left, &input),
                eval(&executable, right, &input),
                eval(&executable, result, &input),
                0,
                flags::ARITHMETIC,
            );
            let value = match kind {
                BinaryKind::Add => a.wrapping_add(b),
                BinaryKind::Sub | BinaryKind::Cmp => a.wrapping_sub(b),
                BinaryKind::And | BinaryKind::Test => a & b,
                BinaryKind::Or => a | b,
                BinaryKind::Xor => a ^ b,
                _ => unreachable!(),
            };
            assert_eq!(actual, oracle_flags(0x403, kind, a, b, value));
            assert_eq!(
                recover(&executable, &run.after, &input)[0],
                if matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                    a
                } else {
                    value
                }
            );
        }
    }
}

#[test]
fn block_parameters_and_recovery_ids_are_actual_inputs_not_predicted_constants() {
    let region = Region::from_blocks(
        &[
            block(0x1000, &[mov(Register32::Eax, Value32::Immediate(7))]),
            block(
                0x1001,
                &[binary(
                    BinaryKind::Add,
                    Register32::Eax,
                    Value32::Immediate(1),
                )],
            ),
        ],
        0x1000,
    )
    .unwrap();
    let values = RegionPlan::analyze(&region);
    let executable = ExecutablePlan::analyze(&region, &values);
    let entry = &executable.block(BlockId(1)).entry;
    assert!(matches!(
        executable.expression(entry.gpr[0]),
        Expr32::Input {
            block: BlockId(1),
            prefix: 0,
            register: 0
        }
    ));
    assert_eq!(entry.frame, values.block(BlockId(1)).entry);
    assert_eq!(
        recover(
            &executable,
            &executable.block(BlockId(1)).exit,
            &[99, 0, 0, 0, 0, 0, 0, 0]
        )[0],
        100
    );
}

fn eval_effects(
    plan: &ExecutableProgram,
    id: ExprId,
    input: &[u32; 8],
    outputs: &BTreeMap<(ExecEffectId, u8), u32>,
) -> u32 {
    match *plan.expression(id) {
        Expr32::Input { register, .. } => input[register as usize],
        Expr32::EffectResult { effect, register } => outputs[&(effect, register)],
        Expr32::Constant(value) => value,
        Expr32::Binary { kind, left, right } => {
            let a = eval_effects(plan, left, input, outputs);
            let b = eval_effects(plan, right, input, outputs);
            match kind {
                Binary32::Add => a.wrapping_add(b),
                Binary32::Sub => a.wrapping_sub(b),
                Binary32::And => a & b,
                Binary32::Or => a | b,
                Binary32::Xor => a ^ b,
            }
        }
        Expr32::ShiftLeft { value, amount } => {
            eval_effects(plan, value, input, outputs).wrapping_shl(u32::from(amount))
        }
    }
}

#[test]
fn ordered_reads_preserve_live_scalar_values_and_all_exact_fault_recoveries() {
    let mut operations = Vec::new();
    for index in 0..128 {
        operations.push(Operation::Lea {
            destination: Register32::Eax,
            address: EffectiveAddress {
                base: Some(Register32::Eax),
                displacement: 1,
                ..Default::default()
            },
        });
        operations.push(mov(
            Register32::Edx,
            Value32::Memory(EffectiveAddress {
                displacement: index * 4,
                ..address()
            }),
        ));
    }
    let (_, executable) = plan(&operations);
    let block = executable.block(BlockId(0));
    let effects: Vec<_> = block.effects().collect();
    assert_eq!(effects.len(), 128);
    let input = [0xffff_ffc0, 11, 0xfeed, 0x3000, 0x4800, 7, 9, 13];
    let mut outputs = BTreeMap::new();
    for (index, effect) in effects.iter().enumerate() {
        let EffectKind::PlainRead32 { address, result } = &effect.kind else {
            panic!("plain read is an explicit ordered effect");
        };
        assert_eq!(effect.point.prefix, index as u32 * 2 + 1);
        assert_eq!(effect.contract.gpr_reads, 1 << Register32::Ebx.index());
        assert_eq!(effect.contract.gpr_writes, 1 << Register32::Edx.index());
        assert_eq!(effect.before.flags, block.entry.flags);
        assert_eq!(effect.after.flags, effect.before.flags);
        for register in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(effect.before.gpr[register], effect.after.gpr[register]);
        }
        assert_eq!(effect.after.gpr[2], *result);
        assert_eq!(
            eval_effects(&executable, address.offset, &input, &outputs),
            0x3000 + index as u32 * 4
        );
        let before = effect
            .before
            .gpr
            .map(|id| eval_effects(&executable, id, &input, &outputs));
        assert_eq!(before[0], input[0].wrapping_add(index as u32 + 1));
        assert_eq!(
            before[2],
            if index == 0 {
                input[2]
            } else {
                0x9000 + index as u32 - 1
            }
        );
        assert_eq!(effect.before.prefix, index as u32 * 2 + 1);
        outputs.insert((effect.id, 2), 0x9000 + index as u32);
    }
    let demanded = executable.dependency_order(&block.exit.scalar_roots());
    assert_eq!(
        demanded
            .iter()
            .filter(|&&id| matches!(
                executable.expression(id),
                Expr32::Binary {
                    kind: Binary32::Add,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        demanded
            .iter()
            .filter(|&&id| matches!(executable.expression(id), Expr32::EffectResult { .. }))
            .count(),
        1
    );
    let final_gpr = block
        .exit
        .gpr
        .map(|id| eval_effects(&executable, id, &input, &outputs));
    assert_eq!(final_gpr, [0x40, 11, 0x907f, 0x3000, 0x4800, 7, 9, 13]);
}

#[test]
fn stores_and_carry_kernels_consume_only_inputs_and_define_only_real_outputs() {
    let store = Operation::Move {
        destination: Location32::Memory(address()),
        source: Value32::Register(Register32::Eax),
    };
    let operations = [
        binary(BinaryKind::Add, Register32::Eax, Value32::Immediate(1)),
        mov(Register32::Edx, Value32::Memory(address())),
        store,
        Operation::Carry {
            kind: CarryKind::Set,
        },
        binary(BinaryKind::Adc, Register32::Ecx, Value32::Immediate(0)),
        Operation::Direction { set: true },
        mov(Register32::Edi, Value32::Memory(address())),
    ];
    let (_, executable) = plan(&operations);
    let block = executable.block(BlockId(0));
    let effects: Vec<_> = block.effects().collect();
    assert_eq!(effects.len(), 6);
    assert_eq!(effects[0].before.flags, effects[1].after.flags);
    assert!(matches!(
        effects[1].before.flags.recipe,
        FlagsRecipe::Arithmetic {
            kind: FlagKind::Add,
            ..
        }
    ));
    assert_eq!(
        effects[1].contract.gpr_reads,
        (1 << Register32::Eax.index()) | (1 << Register32::Ebx.index())
    );
    assert_eq!(effects[1].contract.gpr_writes, 0);
    assert_eq!(effects[1].before.gpr, effects[1].after.gpr);
    assert_eq!(effects[2].contract.gpr_reads, 0);
    assert_eq!(effects[2].contract.gpr_writes, 0);
    assert_eq!(effects[2].contract.flags_write, flags::CF);
    assert_eq!(effects[3].contract.gpr_reads, 1 << Register32::Ecx.index());
    assert_eq!(effects[3].contract.gpr_writes, 1 << Register32::Ecx.index());
    assert_eq!(effects[3].contract.flags_read, flags::CF);
    assert_eq!(
        effects[3].before.flags.origin,
        FlagsOrigin::Effect(effects[2].id)
    );
    assert_eq!(
        effects[3].after.flags.origin,
        FlagsOrigin::Effect(effects[3].id)
    );
    assert_eq!(effects[4].contract.flags_write, 0x400);
    assert_eq!(
        effects[5].before.flags.origin,
        FlagsOrigin::Effect(effects[4].id)
    );
    assert_eq!(effects[5].after.flags, effects[5].before.flags);
    for effect in effects {
        for register in 0..8 {
            if effect.contract.gpr_writes & (1 << register) == 0 {
                assert_eq!(effect.before.gpr[register], effect.after.gpr[register]);
            }
        }
    }
}

#[test]
fn address_inputs_are_before_state_even_when_destination_overwrites_its_base() {
    let (_, executable) = plan(&[
        Operation::Lea {
            destination: Register32::Ebx,
            address: EffectiveAddress {
                displacement: 12,
                ..address()
            },
        },
        mov(Register32::Ebx, Value32::Memory(address())),
        mov(Register32::Eax, Value32::Memory(address())),
    ]);
    let block = executable.block(BlockId(0));
    let effects: Vec<_> = block.effects().collect();
    let EffectKind::PlainRead32 { address: first, .. } = &effects[0].kind else {
        panic!("read");
    };
    let EffectKind::PlainRead32 {
        address: second, ..
    } = &effects[1].kind
    else {
        panic!("read");
    };
    assert_eq!(first.base, Some(effects[0].before.gpr[3]));
    assert_eq!(second.base, Some(effects[0].after.gpr[3]));
    assert_ne!(first.base, second.base);
    assert_eq!(effects[0].after.gpr[0], effects[0].before.gpr[0]);
    assert_eq!(effects[1].after.gpr[3], effects[1].before.gpr[3]);
}

#[test]
fn exact_contracts_distinguish_partial_flags_fp_observers_and_opaque_kernels() {
    use crate::cpu::x86::{X87Condition, X87Operation};
    use effects::{FaultRecovery, classify};

    let partial = classify(
        Some(Operation::MoveByte {
            destination: ByteRegister::Ah,
            source: ByteValue::Immediate(9),
        }),
        None,
    );
    assert_eq!((partial.gpr_reads, partial.gpr_writes), (1, 1));
    assert_eq!((partial.flags_read, partial.flags_write), (0, 0));
    assert_eq!(partial.fp, FpEffectClass::Preserve);

    let increment = classify(
        Some(Operation::Unary {
            kind: UnaryKind::Inc,
            destination: Location32::Register(Register32::Ecx),
        }),
        None,
    );
    assert_eq!(increment.flags_write, flags::ARITHMETIC & !flags::CF);
    assert_eq!(increment.flags_write & flags::CF, 0);

    let compare = classify(
        Some(Operation::X87 {
            operation: X87Operation::Compare {
                index: 1,
                quiet: true,
                pop: 0,
                integer_flags: true,
            },
            address: None,
            waiting: false,
            opcode: 0x123,
        }),
        None,
    );
    assert_eq!(compare.gpr_writes, 0);
    assert_eq!(compare.flags_write, flags::ARITHMETIC);
    let conditional = classify(
        Some(Operation::X87 {
            operation: X87Operation::ConditionalMove {
                index: 1,
                condition: X87Condition::BelowOrEqual,
            },
            address: None,
            waiting: false,
            opcode: 0x124,
        }),
        None,
    );
    assert_eq!(conditional.flags_read, flags::CF | flags::ZF);
    assert_eq!(conditional.flags_write, 0);

    let cpuid = classify(Some(Operation::CpuId), None);
    assert_eq!((cpuid.gpr_reads, cpuid.gpr_writes), (3, 15));
    assert_eq!(cpuid.fp, FpEffectClass::Preserve);
    let observer = classify(Some(Operation::X87StatusToAx), None);
    assert_eq!((observer.gpr_reads, observer.gpr_writes), (1, 1));
    assert_eq!(observer.fp, FpEffectClass::Observe);
    let opaque = classify(None, Some(0x123));
    assert_eq!((opaque.gpr_reads, opaque.gpr_writes), (255, 255));
    assert_eq!(
        (opaque.flags_read, opaque.flags_write),
        (u32::MAX, u32::MAX)
    );
    assert_eq!(opaque.fp, FpEffectClass::Barrier);
    assert_eq!(opaque.fault_recovery, FaultRecovery::Kernel);
}

#[test]
fn decoded_audited_simd_preserves_fp_without_inventing_scalar_def_use_facts() {
    use crate::{
        cpu::x86::decode::decode_one,
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    // movaps xmm0,xmm1 is the audited packed path; movss remains conservative.
    memory
        .write(
            GuestAddress(0x1000),
            &[0x0f, 0x28, 0xc1, 0xf3, 0x0f, 0x10, 0xc1],
        )
        .unwrap();
    let packed = *decode_one(&memory, GuestAddress(0x1000))
        .unwrap()
        .operation();
    let scalar = *decode_one(&memory, GuestAddress(0x1003))
        .unwrap()
        .operation();
    assert!(matches!(packed, Operation::Simd(_)));
    assert!(crate::cpu::core::emitter::preserves_fp_success(&packed));
    assert!(!crate::cpu::core::emitter::preserves_fp_success(&scalar));
    let packed_contract = effects::classify(Some(packed), None);
    assert_eq!(packed_contract.fp, FpEffectClass::Preserve);
    assert_eq!(
        (packed_contract.gpr_reads, packed_contract.gpr_writes),
        (255, 255)
    );
    assert_eq!(
        (packed_contract.flags_read, packed_contract.flags_write),
        (u32::MAX, u32::MAX)
    );
    assert_eq!(
        effects::classify(Some(scalar), None).fp,
        FpEffectClass::Barrier
    );
    let (_, executable) = plan(&[packed]);
    let effect = executable.block(BlockId(0)).effects().next().unwrap();
    assert_eq!(effect.before.fp.payload, effect.after.fp.payload);
    assert_eq!(effect.before.fp.tags, effect.after.fp.tags);
    assert_eq!(effect.before.fp.control, effect.after.fp.control);
    assert_eq!(effect.before.fp.status.bits, effect.after.fp.status.bits);
    assert_eq!(
        effect.before.fp.status.pending,
        effect.after.fp.status.pending
    );
}
