use super::*;

pub(super) fn analyze(region: &Region) -> RegionPlan {
    let facts = flow::entries(region);
    let mut plan = RegionPlan {
        blocks: Vec::with_capacity(region.blocks.len()),
        access: Vec::new(),
        values: Vec::new(),
        proofs: Vec::new(),
        effects: Vec::new(),
        frames: Vec::new(),
        storage: Vec::new(),
        shapes: Vec::new(),
        frame_shapes: Vec::new(),
        frame_proofs: Vec::new(),
        frame_outputs: Vec::new(),
        shape_index: Default::default(),
        constants: Default::default(),
        constant_facts: Vec::new(),
    };
    for (index, block) in region.blocks.iter().enumerate() {
        let id = BlockId(index);
        let (frame, guards) = fp::entry(&mut plan, id, ResumePc::Exact(block.entry), facts[index]);
        let entry = plan.allocate_frame(frame);
        plan.frame_proofs[entry.0] = guards.clone();
        plan.blocks.push(BlockPlan {
            entry,
            exit: entry,
            instructions: Vec::with_capacity(block.instructions.len()),
            contract: EntryContract {
                frame: entry,
                external: true,
                guards,
                gpr_constants: facts[index].gpr,
            },
            edges: Vec::new(),
        });
    }
    for (index, block) in region.blocks.iter().enumerate() {
        let id = BlockId(index);
        let mut current = plan.block(id).entry;
        let mut available = plan.block(id).contract.guards.clone();
        for instruction in &block.instructions {
            let point = ProgramPoint {
                block: id,
                prefix: instruction.prefix,
            };
            let before = current;
            let mut frame = plan.frame(before).clone();
            let mut shape = plan.shape(before).clone();
            gpr::lower(&mut plan, &mut frame, instruction.operation, point);
            if instruction
                .operation
                .and_then(crate::cpu::core::effects::analyze)
                .is_none_or(|e| e.write != 0)
            {
                frame.flags_effect = plan.allocate_effect(EffectKind::Flags, Some(point));
            }
            let helper = instruction.operation.is_none_or(fp::canonical_numeric);
            if instruction
                .operation
                .is_none_or(|op| crate::cpu::core::effects::memory_barrier(op).1)
                || helper
            {
                frame.memory_effect = plan.allocate_effect(EffectKind::Memory, Some(point));
            }
            let numeric = instruction.operation.and_then(fp::operation);
            let output = if let Some((operation, address, opcode)) = numeric {
                fp::lower(
                    &mut plan,
                    &mut frame,
                    &mut shape,
                    operation,
                    address,
                    opcode,
                    point,
                    &mut available,
                )
            } else {
                if helper {
                    fp::kill(&mut plan, &mut frame, &mut shape, point);
                    available.clear();
                }
                None
            };
            let after = instruction.exits.after.unwrap_or(instruction.exits.before);
            frame.pc = after.pc;
            frame.retired = after.retired;
            shape.tags = std::array::from_fn(|i| plan.tag_recipe(frame.fp.tags[i], shape.cells[i]));
            shape.top_delta = frame.fp.top_delta;
            current = plan.allocate_frame_with_shape(frame, shape);
            plan.frame_outputs[current.0] = output;
            // Only completed-prefix facts survive. A slot's payload remains live even
            // when its tag is empty; the serializer still owns its ghost bytes.
            available.retain(|&id| {
                let proof = plan.proof(id);
                proof.epoch == plan.frame(current).fp.control_epoch
                    && proof.value.is_none_or(|value| {
                        plan.frame(current)
                            .fp
                            .payload
                            .iter()
                            .any(|payload| payload.contains(value))
                    })
                    && proof.kind != ProofKind::Empty
            });
            plan.frame_proofs[current.0] = available.clone();
            plan.blocks[index].instructions.push(InstructionPlan {
                before,
                after: current,
            });
        }
        plan.blocks[index].exit = current;
    }
    // The emitter's architectural GPR locals already are the block parameters. FP
    // storage is transferred by the consumed shape/rebase contract, not fictitious
    // copies between analysis-only ValueIds.
    for (index, block) in region.blocks.iter().enumerate() {
        let frame = plan.blocks[index].exit;
        for &target in &block.successors {
            let fp_rebase = plan.shape(frame) != plan.shape(plan.blocks[target.0].entry);
            plan.blocks[index].edges.push(EdgePlan {
                target,
                frame,
                fp_rebase,
            });
        }
    }
    plan.access = access::analyze(region, &plan);
    plan
}

pub(super) fn definition(
    plan: &mut RegionPlan,
    point: ProgramPoint,
    output: u8,
    ty: ValueType,
) -> ValueId {
    plan.allocate_value(ty, ValueKind::Instruction { point, output })
}
