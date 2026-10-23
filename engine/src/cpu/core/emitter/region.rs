use super::{BlockType, INDEX, Output, REMAIN, StopReason};
use crate::cpu::{
    core::region::{BlockId, Region, Shape, Target, Terminator},
    x86::ir::{Condition, CountBranchKind},
};

const ENTRY_PENDING: u32 = 98;

enum Label {
    Restart,
    Unmatched,
    Forward(Vec<BlockId>),
    Loop(BlockId),
    Dispatch(Vec<BlockId>),
    Other,
}

pub(super) fn emit(out: &mut Output<'_>, graph: &Region) {
    let plan = graph.structure_all();
    out.c()
        .i32_const(1)
        .local_set(ENTRY_PENDING)
        .loop_(BlockType::Empty)
        .block(BlockType::Empty);
    let mut emitter = Emitter {
        out,
        graph,
        labels: vec![Label::Restart, Label::Unmatched],
        current: graph.entry,
        dispatched: plan.shapes.iter().flat_map(dispatch_members).collect(),
    };
    emitter.sequence(&plan.shapes, false);
    emitter.labels.pop();
    emitter.out.c().end();
    // No advertised entry matched. No instruction frame has executed here.
    emitter.out.frame = None;
    emitter.out.before_frame = None;
    emitter.out.after_frame = None;
    emitter.out.c().i32_const(0).local_set(109);
    emitter.out.exit(StopReason::NeedCode, None, 0);
    emitter.out.c().end().unreachable();
}

struct Emitter<'a, 'b> {
    out: &'a mut Output<'b>,
    graph: &'a Region,
    labels: Vec<Label>,
    current: BlockId,
    dispatched: Vec<BlockId>,
}

fn members(shape: &Shape) -> Vec<BlockId> {
    match shape {
        Shape::Block(id) => vec![*id],
        Shape::Loop { body, .. } => body.iter().flat_map(members).collect(),
        Shape::Dispatch { blocks, .. } => blocks.clone(),
    }
}

fn dispatch_members(shape: &Shape) -> Vec<BlockId> {
    match shape {
        Shape::Block(_) => Vec::new(),
        Shape::Loop { body, .. } => body.iter().flat_map(dispatch_members).collect(),
        Shape::Dispatch { blocks, .. } => blocks.clone(),
    }
}

impl Emitter<'_, '_> {
    fn sequence(&mut self, shapes: &[Shape], contained: bool) {
        for shape in shapes.iter().rev() {
            self.out.c().block(BlockType::Empty);
            self.labels.push(Label::Forward(members(shape)));
        }
        if !(contained && shapes.len() == 1) && !shapes.is_empty() {
            // Forward labels already own every destination. Pending entry uses
            // one membership-mapped table; known edges land beyond this router.
            self.out.c().local_get(ENTRY_PENDING).if_(BlockType::Empty);
            self.labels.push(Label::Other);
            let mapped: Vec<_> = shapes
                .iter()
                .enumerate()
                .flat_map(|(shape, body)| {
                    members(body)
                        .into_iter()
                        .map(move |id| (id, shape as u32 + 1))
                })
                .collect();
            self.route(&mapped);
            self.labels.pop();
            self.out.c().end();
        }
        for shape in shapes {
            self.out.c().end();
            self.labels.pop();
            match shape {
                Shape::Block(id) => self.block(*id),
                Shape::Loop { header, body } => {
                    self.out.c().loop_(BlockType::Empty);
                    self.labels.push(Label::Loop(*header));
                    // The outer router (or a proven direct edge) selected this
                    // loop's membership. Preserve pending for an interior entry.
                    self.sequence(body, true);
                    self.labels.pop();
                    self.out.c().end();
                }
                Shape::Dispatch { blocks, .. } => self.dispatch(blocks),
            }
        }
    }

    fn route(&mut self, mapped: &[(BlockId, u32)]) {
        let unmatched = self
            .labels
            .iter()
            .rev()
            .position(|label| matches!(label, Label::Unmatched))
            .expect("entry routing has one shared unmatched label") as u32;
        let first = mapped.iter().map(|(id, _)| id.0).min().unwrap();
        let last = mapped.iter().map(|(id, _)| id.0).max().unwrap();
        // Block IDs need not follow shape order, and separate decode paths may
        // leave holes. Bound the table to this sequence's own member range.
        let mut destinations = vec![unmatched; last - first + 1];
        for &(id, depth) in mapped {
            destinations[id.0 - first] = depth;
        }
        self.out.c().local_get(INDEX);
        if first != 0 {
            self.out.c().i32_const(first as i32).i32_sub();
        }
        self.out.c().br_table(destinations, unmatched);
    }

    fn block(&mut self, id: BlockId) {
        self.current = id;
        self.out.c().i32_const(0).local_set(ENTRY_PENDING);
        let block = &self.graph.blocks[id.0];
        self.out.guarded_control = block.terminator == Terminator::GuardedIndirect;
        self.out.address_fact = None;
        self.out
            .c()
            .local_get(super::ENTRY_OFFSET)
            .i32_eqz()
            .local_get(REMAIN)
            .i32_const(block.instructions.len() as i32)
            .i32_ge_u()
            .i32_and()
            .local_set(super::FULL_BLOCK);
        self.out.frame = Some(self.out.plan.block(id).entry);
        self.out.before_frame = self.out.frame;
        self.out.after_frame = self.out.frame;
        let constants = self.out.plan.block(id).contract.gpr_constants;
        self.out.guarded_gpr_facts = true;
        self.out
            .c()
            .local_get(super::ENTRY_OFFSET)
            .if_(BlockType::Empty);
        super::numeric::enter_port(self.out);
        self.out
            .c()
            .i32_const(0)
            .local_set(super::GP_FACTS_VALID)
            .local_get(super::ENTRY_OFFSET)
            .local_set(super::PREFIX_BASE)
            .else_();
        self.out.c().i32_const(0).local_set(super::PREFIX_BASE);
        // Group proofs consume actual GPR value identities. Ordinary entries
        // without constant guards are valid; interior ports took the false arm.
        self.out.c().i32_const(1);
        for (register, value) in constants.into_iter().enumerate() {
            if let Some(value) = value {
                self.out
                    .c()
                    .local_get(register as u32 + 2)
                    .i32_const(value as i32)
                    .i32_eq()
                    .i32_and();
            }
        }
        self.out.c().local_set(super::GP_FACTS_VALID);
        super::numeric::enter_block(self.out, id);
        self.out.c().end();
        if !self.out.is_trace {
            super::trace::record_block(
                self.out,
                block.entry,
                block
                    .instructions
                    .last()
                    .expect("nonempty region block")
                    .next,
            );
        }
        self.out.c().i32_const(0).local_set(super::ENTRY_OFFSET);
        self.out.block(id, block);
        match block.terminator {
            Terminator::Next { target, .. } => self.edge(target),
            Terminator::Branch {
                condition,
                taken,
                otherwise,
                ..
            } => {
                self.out.jump_condition(condition);
                self.branch(taken, otherwise);
            }
            Terminator::CountBranch {
                kind,
                word,
                taken,
                otherwise,
            } => {
                if kind != CountBranchKind::EcxZero {
                    self.out.c().local_get(3).i32_const(1).i32_sub();
                    if word {
                        self.out
                            .c()
                            .i32_const(0xffff)
                            .i32_and()
                            .local_get(3)
                            .i32_const(!0xffff)
                            .i32_and()
                            .i32_or();
                    }
                    self.out.c().local_set(3);
                }
                self.out.c().local_get(3);
                if word {
                    self.out.c().i32_const(0xffff).i32_and();
                }
                self.out.c().i32_eqz();
                if kind != CountBranchKind::EcxZero {
                    self.out.c().i32_eqz();
                }
                if matches!(
                    kind,
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual
                ) {
                    self.out.c().local_set(super::A);
                    self.out.condition(if kind == CountBranchKind::LoopEqual {
                        Condition::Equal
                    } else {
                        Condition::NotEqual
                    });
                    self.out.c().local_get(super::A).i32_and();
                }
                self.branch(taken, otherwise);
            }
            Terminator::Indirect => self.restart(),
            Terminator::GuardedIndirect => {
                for &target in &block.indirect_targets {
                    self.out
                        .c()
                        .local_get(super::EIP)
                        .i32_const(self.graph.blocks[target.0].entry as i32)
                        .i32_eq()
                        .if_(BlockType::Empty);
                    self.labels.push(Label::Other);
                    self.edge(Target::Block(target));
                    self.labels.pop();
                    self.out.c().end();
                }
                // the actual indirect instruction has already committed its
                // stack/register effects and retirement. only its target missed.
                self.out.exit(StopReason::NeedCode, None, 0);
            }
            Terminator::Exit { .. } => {
                self.out.c().unreachable();
            }
        }
    }

    fn branch(&mut self, taken: Target, otherwise: Target) {
        self.out.c().if_(BlockType::Empty);
        self.labels.push(Label::Other);
        self.edge(taken);
        self.out.c().else_();
        self.edge(otherwise);
        self.labels.pop();
        self.out.c().end();
    }

    fn edge(&mut self, target: Target) {
        let Target::Block(id) = target else {
            let Target::External(pc) = target else {
                unreachable!()
            };
            self.out.exit(StopReason::NeedCode, Some(pc), 0);
            return;
        };
        let edge = self
            .out
            .plan
            .block(self.current)
            .edges
            .iter()
            .find(|edge| edge.target == id)
            .cloned()
            .expect("typed region edge has an analyzed state contract");
        super::numeric::emit_edge(self.out, &edge);
        for (depth, label) in self.labels.iter().rev().enumerate() {
            let pending = match label {
                Label::Forward(ids) if ids.contains(&id) => i32::from(ids.first() != Some(&id)),
                Label::Loop(header) if *header == id => 0,
                Label::Dispatch(ids) if ids.contains(&id) => 0,
                _ => continue,
            };
            if pending != 0 || self.dispatched.contains(&id) {
                self.out.c().i32_const(id.0 as i32).local_set(INDEX);
            }
            self.out
                .c()
                .i32_const(pending)
                .local_set(ENTRY_PENDING)
                .br(depth as u32);
            return;
        }
        let pc = self.graph.blocks[id.0].entry;
        self.out
            .c()
            .i32_const(id.0 as i32)
            .local_set(INDEX)
            .i32_const(pc as i32)
            .local_set(super::EIP);
        self.restart();
    }

    fn restart(&mut self) {
        self.out
            .c()
            .local_get(INDEX)
            .i32_const(-1)
            .i32_eq()
            .if_(BlockType::Empty);
        self.out.exit(StopReason::NeedCode, None, 0);
        self.out.c().end();
        if let Some(frame) = self.out.frame {
            super::numeric::emit_frame(self.out, frame);
        }
        // An unproved computed predecessor commits through the same in-body
        // service as a terminal prefix, then reimports the pool identity frame.
        // The selected INDEX/ENTRY_OFFSET and live scalar lease survive.
        self.out
            .c()
            .i32_const(1)
            .local_set(super::SERVICE_RESUME)
            .i32_const(StopReason::NeedCode as i32)
            .return_();
    }

    fn dispatch(&mut self, blocks: &[BlockId]) {
        self.out.c().loop_(BlockType::Empty);
        self.labels.push(Label::Dispatch(blocks.to_vec()));
        for _ in blocks {
            self.out.c().block(BlockType::Empty);
            self.labels.push(Label::Other);
        }
        let mapped: Vec<_> = blocks
            .iter()
            .copied()
            .enumerate()
            .map(|(index, id)| (id, index as u32))
            .collect();
        self.route(&mapped);
        for id in blocks {
            self.labels.pop();
            self.out.c().end();
            self.block(*id);
        }
        self.out.c().unreachable();
        self.labels.pop();
        self.out.c().end();
    }
}
