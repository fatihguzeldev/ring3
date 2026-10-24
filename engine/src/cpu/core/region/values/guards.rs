//! Successful-fast-path admission schedule. Entry predictions are conditional;
//! only an executed first use or a completed producer establishes a local fact.
use super::*;
use crate::cpu::x86::{X87Binary, X87Format};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod continuation_tests;

#[derive(Clone, Debug, Default)]
pub(in crate::cpu::core) struct InputGuard {
    pub logical: u8,
    pub occupied: bool,
    pub load: bool,
    pub admit: Option<ProofKind>,
    pub format: bool,
}

#[derive(Clone, Debug, Default)]
pub(in crate::cpu::core) struct FpGuardPlan {
    pub enter: bool,
    pub control: bool,
    pub inputs: Vec<InputGuard>,
    pub empty: u8,
    pub exact_mul: bool,
}

pub(in crate::cpu::core) struct Guards {
    frames: Vec<FpGuardPlan>,
    continuations: Vec<Vec<FpContinuation>>,
}

/// The edge itself changes no control word. Its complete payload parameters
/// and independent tags rename successful obligations into the target epoch.
/// This is not a proof granted by a storage layout or an external entry hint.
#[derive(Clone, Debug)]
pub(in crate::cpu::core) struct FpContinuation {
    pub target: BlockId,
    pub source_frame: FrameStateId,
    pub target_frame: FrameStateId,
    pub control: ControlEpochMap,
    pub payload_sources: [StorageCell; 8],
    pub tag_sources: [TagRecipe; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct ControlEpochMap {
    pub source: EffectId,
    pub target: EffectId,
}

#[derive(Default)]
struct Facts {
    entered: bool,
    control: bool,
    loaded: BTreeSet<PayloadKey>,
    format: BTreeSet<PayloadKey>,
    occupied: BTreeSet<ValueId>,
    empty: BTreeSet<ValueId>,
    admitted: BTreeMap<PayloadKey, u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PayloadKey {
    Native(ValueId),
    Opaque(ValueId, ValueId),
    Tagged(ValueId, ValueId, ValueId, ValueId),
}
impl From<FpValue> for PayloadKey {
    fn from(value: FpValue) -> Self {
        match value {
            FpValue::ExactF64(native, _) => Self::Native(native),
            FpValue::Opaque80 { lo, hi } => Self::Opaque(lo, hi),
            FpValue::Tagged {
                lo,
                hi,
                native,
                valid,
                ..
            } => Self::Tagged(lo, hi, native, valid),
        }
    }
}

fn bit(kind: ProofKind) -> u8 {
    match kind {
        ProofKind::ExactF64 => 1,
        ProofKind::BoundedF64 => 2,
        ProofKind::Native24 => 4,
        ProofKind::Native24InSingle => 8,
        _ => 0,
    }
}
fn implied(kind: ProofKind) -> u8 {
    match kind {
        ProofKind::Native24 => 15,
        ProofKind::Native24InSingle => 11,
        ProofKind::BoundedF64 => 3,
        ProofKind::ExactF64 => 1,
        _ => 0,
    }
}
fn proof_bits(plan: &RegionPlan, frame: FrameStateId, logical: u8) -> u8 {
    let state = plan.frame(frame);
    let payload = state.fp.payload[logical as usize];
    plan.proofs_at(frame)
        .iter()
        .map(|&id| plan.proof(id))
        .filter(|proof| {
            proof.conditional.is_none()
                && proof.epoch == state.fp.control_epoch
                && proof.value.is_some_and(|value| payload.contains(value))
        })
        .fold(0, |bits, proof| bits | bit(proof.kind))
}
fn strongest(bits: u8) -> Option<ProofKind> {
    if bits & 4 != 0 {
        Some(ProofKind::Native24)
    } else if bits & 8 != 0 {
        Some(ProofKind::Native24InSingle)
    } else if bits & 2 != 0 {
        Some(ProofKind::BoundedF64)
    } else if bits & 1 != 0 {
        Some(ProofKind::ExactF64)
    } else {
        None
    }
}
fn tag_input(plan: &RegionPlan, frame: FrameStateId, logical: u8) -> Option<ValueId> {
    match plan.frame(frame).fp.tags[logical as usize] {
        TagFact::Unknown(value) => Some(value),
        _ => None,
    }
}
fn remember_admission(facts: &mut Facts, payload: FpValue, bits: u8) {
    *facts.admitted.entry(payload.into()).or_default() |= bits;
}

impl Guards {
    pub fn at(&self, frame: FrameStateId) -> &FpGuardPlan {
        &self.frames[frame.0]
    }

    pub fn continuation(&self, source: BlockId, target: BlockId) -> &FpContinuation {
        self.continuations[source.0]
            .iter()
            .find(|edge| edge.target == target)
            .expect("FP continuation has an analyzed CFG edge")
    }

    pub fn analyze(graph: &Region, plan: &RegionPlan) -> Self {
        let mut result = Self {
            frames: vec![FpGuardPlan::default(); plan.frames().len()],
            continuations: graph
                .blocks
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let source = plan.block(BlockId(index));
                    source
                        .edges
                        .iter()
                        .map(|edge| {
                            let target_frame = plan.block(edge.target).entry;
                            let from = plan.frame(edge.frame);
                            let to = plan.frame(target_frame);
                            FpContinuation {
                                target: edge.target,
                                source_frame: edge.frame,
                                target_frame,
                                control: ControlEpochMap {
                                    source: from.fp.control_epoch,
                                    target: to.fp.control_epoch,
                                },
                                payload_sources: std::array::from_fn(|logical| {
                                    let cell = edge.fp_transfer.sources[logical];
                                    for value in from.fp.payload[logical].values() {
                                        assert_eq!(plan.storage(value).unwrap().cell, cell);
                                    }
                                    for value in to.fp.payload[logical].values() {
                                        assert_eq!(
                                            plan.storage(value).unwrap().cell,
                                            StorageCell(logical as u8)
                                        );
                                    }
                                    cell
                                }),
                                tag_sources: edge.fp_transfer.tags,
                            }
                        })
                        .collect()
                })
                .collect(),
        };
        for (index, block) in graph.blocks.iter().enumerate() {
            // Every block is an advertised external entry. The first actual use
            // retains conditional adapters; prepared edges reuse their runtime
            // masks. After that use, dominance is a compile-time fact.
            let mut facts = Facts::default();
            for instruction in &block.instructions {
                let step = plan.instruction(BlockId(index), instruction.prefix);
                let Some(operation) = instruction.operation else {
                    facts = Facts::default();
                    continue;
                };
                let Some((operation, ..)) = fp::operation(operation) else {
                    if fp::canonical_numeric(operation) {
                        facts = Facts::default();
                    }
                    continue;
                };
                if matches!(operation, X87Operation::StoreControl) {
                    continue;
                }
                if matches!(operation, X87Operation::LoadControl) {
                    // A successful CW load preserves the actual payload/tag
                    // identities, but no environment/domain admission crosses
                    // its epoch. The next use keeps its runtime authority test.
                    facts.entered = false;
                    facts.control = false;
                    facts.admitted.clear();
                    continue;
                }
                if !flow::typed_fp(operation) {
                    if !matches!(operation, X87Operation::Wait | X87Operation::StatusToAx) {
                        facts = Facts::default();
                    }
                    continue;
                }
                let arithmetic = matches!(
                    operation,
                    X87Operation::Binary { .. } | X87Operation::BinaryMemory { .. }
                );
                let format = !matches!(operation, X87Operation::Store { .. });
                let mut reads = match operation {
                    X87Operation::LoadRegister { index } => vec![index],
                    X87Operation::Exchange { index } => vec![0, index],
                    X87Operation::Binary {
                        destination,
                        source,
                        ..
                    } => vec![destination, source],
                    X87Operation::Load(
                        X87Format::Float32
                        | X87Format::Integer16
                        | X87Format::Integer32
                        | X87Format::Integer64,
                    ) => vec![],
                    _ => vec![0],
                };
                // Admission can use arithmetic scratch. All distinct inputs are
                // prepared before either FA/FB is captured, including ST(i),ST(i).
                let mut seen = 0u8;
                reads.retain(|&logical| {
                    let fresh = seen & (1 << logical) == 0;
                    seen |= 1 << logical;
                    fresh
                });
                let mut guard = FpGuardPlan {
                    enter: !facts.entered,
                    control: arithmetic && !facts.control,
                    ..Default::default()
                };
                facts.entered = true;
                facts.control |= arithmetic;
                for logical in reads {
                    let payload = plan.frame(step.before).fp.payload[logical as usize];
                    let key = PayloadKey::from(payload);
                    let occupied = if let Some(tag) = tag_input(plan, step.before, logical) {
                        facts.occupied.insert(tag)
                    } else {
                        matches!(
                            plan.frame(step.before).fp.tags[logical as usize],
                            TagFact::Empty
                        )
                    };
                    let load = !facts.loaded.contains(&key);
                    // Arithmetic consumes one exact bounded native operand,
                    // with the PC24 precision implication, even at an entirely
                    // independent owner entry. A producer's physical certificate
                    // can satisfy this first use without repeating its guards.
                    let expected =
                        proof_bits(plan, step.before, logical) | if arithmetic { 11 } else { 0 };
                    let established = facts.admitted.get(&key).copied().unwrap_or(0);
                    let admit = strongest(expected).filter(|kind| established & bit(*kind) == 0);
                    let classified = facts.format.contains(&key);
                    let check_format = format
                        && !classified
                        && admit.is_none()
                        && !matches!(payload, FpValue::ExactF64(..));
                    guard.inputs.push(InputGuard {
                        logical,
                        occupied,
                        load,
                        admit,
                        format: check_format,
                    });
                    facts.loaded.insert(key);
                    if check_format || admit.is_some() || matches!(payload, FpValue::ExactF64(..)) {
                        facts.format.insert(key);
                    }
                    if let Some(kind) = admit {
                        remember_admission(&mut facts, payload, implied(kind));
                    }
                    if arithmetic {
                        remember_admission(&mut facts, payload, 11);
                        // Successful operand admission explicitly defines this
                        // native lane. No other shared payload component grants
                        // an identity or representation fact.
                        let native = match payload {
                            FpValue::ExactF64(value, _) | FpValue::Tagged { native: value, .. } => {
                                Some(value)
                            }
                            FpValue::Opaque80 { .. } => None,
                        };
                        if let Some(native) = native {
                            let key = PayloadKey::Native(native);
                            facts.loaded.insert(key);
                            facts.format.insert(key);
                            *facts.admitted.entry(key).or_default() |= 11;
                        }
                    }
                }
                if matches!(
                    operation,
                    X87Operation::Load(
                        X87Format::Float32
                            | X87Format::Integer16
                            | X87Format::Integer32
                            | X87Format::Integer64
                    ) | X87Operation::LoadRegister { .. }
                ) {
                    if let Some(tag) = tag_input(plan, step.before, 7) {
                        if facts.empty.insert(tag) {
                            guard.empty |= 1 << 7;
                        }
                    } else if !matches!(plan.frame(step.before).fp.tags[7], TagFact::Empty) {
                        guard.empty |= 1 << 7;
                    }
                }
                guard.exact_mul = matches!(
                    operation,
                    X87Operation::Binary {
                        kind: X87Binary::Multiply,
                        ..
                    }
                ) && match operation {
                    X87Operation::Binary {
                        destination,
                        source,
                        ..
                    } => {
                        proof_bits(plan, step.before, destination) & 4 != 0
                            && proof_bits(plan, step.before, source) & 4 != 0
                    }
                    _ => false,
                };
                result.frames[step.before.0] = guard;
                // Only the actual output cell is produced. Raw sign changes
                // define a complete tagged native-or-opaque representation, not
                // every individual lane; their validated input format survives.
                if let Some(output) = plan.fp_output_cell(step.after) {
                    for logical in 0..8u8 {
                        if plan.shape(step.after).cells[logical as usize] != output {
                            continue;
                        }
                        let payload = plan.frame(step.after).fp.payload[logical as usize];
                        let key = PayloadKey::from(payload);
                        facts.loaded.insert(key);
                        facts.format.insert(key);
                        remember_admission(
                            &mut facts,
                            payload,
                            proof_bits(plan, step.after, logical),
                        );
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::core::program;
    use crate::cpu::x86::{
        Register32,
        ir::{BranchTarget, Location32, Operation, Value32},
        simd::{SimdKind, SimdOperand, SimdOperation, SimdRegister},
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
            waiting: false,
            opcode: 0x123,
        }
    }

    fn binary(kind: X87Binary, source: u8) -> Operation {
        fp(X87Operation::Binary {
            kind,
            destination: 0,
            source,
            pop: false,
        })
    }

    fn simd(scalar: bool) -> Operation {
        Operation::Simd(SimdOperation {
            kind: SimdKind::Move,
            destination: SimdOperand::Register(SimdRegister::Xmm(0)),
            source: SimdOperand::Register(SimdRegister::Xmm(1)),
            vector_bytes: 16,
            lane_bytes: 16,
            memory_bytes: 0,
            immediate: None,
            aligned: false,
            scalar,
            implicit_address: None,
        })
    }

    fn analyze(operations: &[Operation]) -> (RegionPlan, Guards) {
        let graph = Region::from_blocks(&[block(0x1000, operations)], 0x1000).unwrap();
        let values = RegionPlan::analyze(&graph);
        let guards = Guards::analyze(&graph, &values);
        (values, guards)
    }

    fn at<'a>(values: &RegionPlan, guards: &'a Guards, prefix: u32) -> &'a FpGuardPlan {
        guards.at(values.instruction(BlockId(0), prefix).before)
    }

    #[test]
    fn successful_admissions_dominate_mixed_integer_simd_and_status_preservers() {
        let (values, guards) = analyze(&[
            binary(X87Binary::Add, 1),
            Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(7),
            },
            simd(false),
            Operation::X87StatusToAx,
            binary(X87Binary::Add, 1),
            binary(X87Binary::Add, 1),
        ]);
        let first = at(&values, &guards, 0);
        assert!(first.enter && first.control);
        assert_eq!(first.inputs.len(), 2);
        assert!(first.inputs.iter().all(|input| {
            input.occupied
                && input.load
                && !input.format
                && input.admit == Some(ProofKind::Native24InSingle)
        }));
        for prefix in [4, 5] {
            let later = at(&values, &guards, prefix);
            assert!(!later.enter && !later.control);
            assert_eq!(later.inputs.len(), 2);
            assert!(later.inputs.iter().all(|input| {
                !input.occupied && !input.load && !input.format && input.admit.is_none()
            }));
        }
    }

    #[test]
    fn control_changes_recheck_environment_without_reimporting_unchanged_payloads() {
        let (values, guards) = analyze(&[
            binary(X87Binary::Add, 1),
            fp(X87Operation::LoadControl),
            binary(X87Binary::Add, 1),
        ]);
        let later = at(&values, &guards, 2);
        assert!(later.enter && later.control);
        assert!(later.inputs.iter().all(|input| !input.occupied
            && !input.load
            && !input.format
            && input.admit == Some(ProofKind::Native24InSingle)));
        let first = values.instruction(BlockId(0), 0).before;
        let last = values.instruction(BlockId(0), 2).before;
        assert_ne!(
            values.frame(first).fp.control_epoch,
            values.frame(last).fp.control_epoch
        );
    }

    #[test]
    fn a_real_canonical_barrier_reimports_all_actual_inputs() {
        let (values, guards) = analyze(&[
            binary(X87Binary::Add, 1),
            Operation::InitializeX87,
            binary(X87Binary::Add, 1),
        ]);
        let later = at(&values, &guards, 2);
        assert!(later.enter && later.control);
        assert!(later.inputs.iter().all(|input| input.occupied
            && input.load
            && !input.format
            && input.admit == Some(ProofKind::Native24InSingle)));
    }

    #[test]
    fn same_epoch_numeric_effects_schedule_only_the_first_successful_environment_guard() {
        let (values, guards) = analyze(&[
            fp(X87Operation::Load(X87Format::Integer32)),
            binary(X87Binary::Add, 1),
            binary(X87Binary::Add, 1),
            fp(X87Operation::StoreControl),
            binary(X87Binary::Add, 1),
            fp(X87Operation::LoadControl),
            binary(X87Binary::Add, 1),
            binary(X87Binary::Add, 1),
        ]);
        assert!(at(&values, &guards, 0).enter);
        assert!(!at(&values, &guards, 1).enter && at(&values, &guards, 1).control);
        for prefix in [2, 4, 7] {
            assert!(!at(&values, &guards, prefix).enter && !at(&values, &guards, prefix).control);
        }
        assert!(at(&values, &guards, 6).enter && at(&values, &guards, 6).control);
    }

    #[test]
    fn identical_binary_inputs_have_one_pre_capture_admission() {
        let (values, guards) = analyze(&[binary(X87Binary::Add, 0)]);
        let first = at(&values, &guards, 0);
        assert_eq!(first.inputs.len(), 1);
        assert_eq!(first.inputs[0].logical, 0);
        assert!(first.inputs[0].load && !first.inputs[0].format);
        assert_eq!(first.inputs[0].admit, Some(ProofKind::Native24InSingle));
    }

    #[test]
    fn loading_an_opaque_store_input_does_not_classify_its_later_arithmetic_use() {
        let (values, guards) = analyze(&[
            fp(X87Operation::Store {
                format: X87Format::Float64,
                pop: false,
                truncate: false,
            }),
            binary(X87Binary::Add, 1),
        ]);
        let store = at(&values, &guards, 0);
        assert!(store.inputs[0].load && !store.inputs[0].format);
        let arithmetic = at(&values, &guards, 1);
        assert!(arithmetic.control);
        assert!(!arithmetic.inputs[0].load);
        assert!(!arithmetic.inputs[0].format);
        assert_eq!(
            arithmetic.inputs[0].admit,
            Some(ProofKind::Native24InSingle)
        );
        assert!(!arithmetic.inputs[0].occupied);
    }

    #[test]
    fn representation_facts_require_the_whole_payload_identity() {
        let original = FpValue::Tagged {
            lo: ValueId(1),
            hi: ValueId(2),
            native: ValueId(3),
            valid: ValueId(4),
            proof: ProofId(0),
        };
        let different_high = FpValue::Tagged {
            lo: ValueId(1),
            hi: ValueId(9),
            native: ValueId(3),
            valid: ValueId(4),
            proof: ProofId(0),
        };
        let mut facts = Facts::default();
        facts.loaded.insert(original.into());
        facts.format.insert(original.into());
        remember_admission(&mut facts, original, 7);
        for other in [
            different_high,
            FpValue::Opaque80 {
                lo: ValueId(1),
                hi: ValueId(2),
            },
            FpValue::ExactF64(ValueId(3), ProofId(0)),
        ] {
            let key = PayloadKey::from(other);
            assert!(!facts.loaded.contains(&key));
            assert!(!facts.format.contains(&key));
            assert!(!facts.admitted.contains_key(&key));
        }
    }

    #[test]
    fn aliases_reuse_value_admission_but_empty_tokens_follow_the_physical_destination() {
        let (values, guards) = analyze(&[
            fp(X87Operation::LoadRegister { index: 0 }),
            fp(X87Operation::LoadRegister { index: 1 }),
        ]);
        let first = at(&values, &guards, 0);
        assert!(first.inputs[0].occupied && first.inputs[0].load && first.inputs[0].format);
        let second = at(&values, &guards, 1);
        assert_eq!(second.inputs.len(), 1);
        assert!(!second.inputs[0].occupied);
        assert!(!second.inputs[0].load && !second.inputs[0].format);
        assert_eq!(first.empty, 0x80);
        assert_eq!(second.empty, 0x80);
        let before = values.instruction(BlockId(0), 1).before;
        assert_eq!(
            values.frame(before).fp.payload[0],
            values.frame(before).fp.payload[1]
        );
        assert_ne!(
            values.frame(before).fp.tags[0],
            values.frame(before).fp.tags[1],
            "a newly classified destination does not rewrite its aliased source tag"
        );
    }

    #[test]
    fn exact_product_requires_two_actual_native24_inputs_in_the_same_epoch() {
        let load = fp(X87Operation::Load(X87Format::Float32));
        let multiply = binary(X87Binary::Multiply, 1);
        let (values, guards) = analyze(&[load, load, multiply]);
        let product = at(&values, &guards, 2);
        assert!(product.exact_mul && product.control);
        assert!(
            product
                .inputs
                .iter()
                .all(|input| { !input.load && !input.format && input.admit.is_none() })
        );
        for operations in [
            vec![load, multiply],
            vec![binary(X87Binary::Add, 1), multiply],
            vec![load, load, fp(X87Operation::LoadControl), multiply],
        ] {
            let (values, guards) = analyze(&operations);
            let product = at(&values, &guards, operations.len() as u32 - 1);
            assert!(!product.exact_mul);
        }
    }

    #[test]
    fn incoming_contract_facts_keep_the_conditional_first_use_for_every_external_entry() {
        let load = fp(X87Operation::Load(X87Format::Float32));
        let graph = Region::from_blocks(
            &[
                block(
                    0x1000,
                    &[
                        load,
                        load,
                        Operation::Jump {
                            target: BranchTarget::Direct(GuestAddress(0x2000)),
                        },
                    ],
                ),
                block(0x2000, &[binary(X87Binary::Multiply, 1)]),
            ],
            0x1000,
        )
        .unwrap();
        let values = RegionPlan::analyze(&graph);
        let guards = Guards::analyze(&graph, &values);
        let target = guards.at(values.block(BlockId(1)).entry);
        assert!(target.enter && target.control && target.exact_mul);
        assert_eq!(target.inputs.len(), 2);
        assert!(target.inputs.iter().all(|input| {
            input.occupied && input.load && input.admit == Some(ProofKind::Native24)
        }));
    }

    #[test]
    fn first_use_is_not_hoisted_to_an_earlier_integer_prefix_or_unused_ghost() {
        let (values, guards) = analyze(&[
            Operation::Nop,
            Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(7),
            },
            fp(X87Operation::LoadRegister { index: 4 }),
        ]);
        for prefix in [0, 1] {
            let integer = at(&values, &guards, prefix);
            assert!(!integer.enter && !integer.control && integer.inputs.is_empty());
        }
        let first = at(&values, &guards, 2);
        assert!(first.enter && !first.control);
        assert_eq!(first.inputs.len(), 1);
        assert_eq!(first.inputs[0].logical, 4);
    }
}
