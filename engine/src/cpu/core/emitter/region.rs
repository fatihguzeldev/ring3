use super::{BlockType, INDEX, Output, REMAIN, StopReason};
use crate::cpu::{
    core::region::{BlockId, Region, Shape, Target, Terminator},
    x86::ir::{Condition, CountBranchKind},
};

const ENTRY_PENDING: u32 = 98;

enum Label {
    Restart,
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
        .loop_(BlockType::Empty);
    let mut emitter = Emitter {
        out,
        graph,
        labels: vec![Label::Restart],
        current: graph.entry,
        dispatched: plan.shapes.iter().flat_map(dispatch_members).collect(),
    };
    emitter.sequence(&plan.shapes);
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
    fn sequence(&mut self, shapes: &[Shape]) {
        for shape in shapes.iter().rev() {
            self.out.c().block(BlockType::Empty);
            self.labels.push(Label::Forward(members(shape)));
        }
        for shape in shapes {
            self.out.c().end();
            self.labels.pop();
            match shape {
                Shape::Block(id) => self.block(*id, true),
                Shape::Loop { header, body } => {
                    self.out.c().loop_(BlockType::Empty);
                    self.labels.push(Label::Loop(*header));
                    self.sequence(body);
                    self.labels.pop();
                    self.out.c().end();
                }
                Shape::Dispatch { blocks, .. } => self.dispatch(blocks),
            }
        }
    }

    fn block(&mut self, id: BlockId, entry_guard: bool) {
        self.current = id;
        if entry_guard {
            self.out
                .c()
                .local_get(ENTRY_PENDING)
                .i32_eqz()
                .local_get(INDEX)
                .i32_const(id.0 as i32)
                .i32_eq()
                .i32_or()
                .if_(BlockType::Empty);
            self.labels.push(Label::Other);
        }
        self.out.c().i32_const(0).local_set(ENTRY_PENDING);
        let block = &self.graph.blocks[id.0];
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
            Terminator::Exit { .. } => {
                self.out.c().unreachable();
            }
        }
        if entry_guard {
            self.labels.pop();
            self.out.c().end();
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
        self.out.c().local_get(ENTRY_PENDING).i32_eqz();
        for id in blocks {
            self.out
                .c()
                .local_get(INDEX)
                .i32_const(id.0 as i32)
                .i32_eq()
                .i32_or();
        }
        self.out.c().if_(BlockType::Empty).loop_(BlockType::Empty);
        self.labels.push(Label::Other);
        self.labels.push(Label::Dispatch(blocks.to_vec()));
        self.out.c().block(BlockType::Empty);
        self.labels.push(Label::Other);
        for _ in blocks {
            self.out.c().block(BlockType::Empty);
            self.labels.push(Label::Other);
        }
        let destinations: Vec<_> = (0..self.graph.blocks.len())
            .map(|id| {
                blocks
                    .iter()
                    .position(|block| block.0 == id)
                    .map_or(blocks.len() as u32, |index| index as u32)
            })
            .collect();
        self.out
            .c()
            .local_get(INDEX)
            .br_table(destinations, blocks.len() as u32);
        for id in blocks {
            self.labels.pop();
            self.out.c().end();
            self.block(*id, false);
        }
        self.labels.pop();
        self.out.c().end();
        self.restart();
        self.labels.pop();
        self.labels.pop();
        self.out.c().end().end();
    }
}
