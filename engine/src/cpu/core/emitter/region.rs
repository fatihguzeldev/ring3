use super::{BlockType, INDEX, Output, REMAIN, StopReason};
use crate::cpu::core::region::{BlockId, Region, Shape, Target, Terminator};

const ENTRY_PENDING: u32 = 98;

enum Label {
    Restart,
    Forward(Vec<BlockId>),
    Loop(BlockId),
    Dispatch(Vec<BlockId>),
    Other,
}

pub(super) fn emit(out: &mut Output<'_>, graph: &Region, precise: bool) {
    let plan = graph.structure_all();
    out.c()
        .i32_const(1)
        .local_set(ENTRY_PENDING)
        .loop_(BlockType::Empty);
    let mut emitter = Emitter {
        out,
        graph,
        precise,
        labels: vec![Label::Restart],
    };
    emitter.sequence(&plan.shapes);
    emitter.out.exit(StopReason::NeedCode, None, 0);
    emitter.out.c().end().unreachable();
}

struct Emitter<'a, 'b> {
    out: &'a mut Output<'b>,
    graph: &'a Region,
    precise: bool,
    labels: Vec<Label>,
}

fn members(shape: &Shape) -> Vec<BlockId> {
    match shape {
        Shape::Block(id) => vec![*id],
        Shape::Loop { body, .. } => body.iter().flat_map(members).collect(),
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
        if !self.precise {
            self.out
                .c()
                .local_get(REMAIN)
                .i32_const(block.instructions.len() as i32)
                .i32_lt_u()
                .if_(BlockType::Empty);
            self.out.publish_exit(Some(block.entry), 0);
            self.out
                .c()
                .i32_const(super::exits::PRECISE)
                .return_()
                .end();
        }
        self.out.block(block, self.precise);
        match block.terminator {
            Terminator::Next { target, .. } => self.edge(target),
            Terminator::Branch {
                taken, otherwise, ..
            }
            | Terminator::CountBranch {
                taken, otherwise, ..
            } => {
                let index = match taken {
                    Target::Block(id) => id.0 as i32,
                    Target::External(_) => -1,
                };
                // External arms can share INDEX=-1; EIP distinguishes their exact PC.
                if let Target::External(pc) = taken {
                    self.out
                        .c()
                        .local_get(super::EIP)
                        .i32_const(pc as i32)
                        .i32_eq();
                } else {
                    self.out.c().local_get(INDEX).i32_const(index).i32_eq();
                }
                self.out.c().if_(BlockType::Empty);
                self.labels.push(Label::Other);
                self.edge(taken);
                self.out.c().else_();
                self.edge(otherwise);
                self.labels.pop();
                self.out.c().end();
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

    fn edge(&mut self, target: Target) {
        let Target::Block(id) = target else {
            self.out.exit(StopReason::NeedCode, None, 0);
            return;
        };
        for (depth, label) in self.labels.iter().rev().enumerate() {
            let pending = match label {
                Label::Forward(ids) if ids.contains(&id) => i32::from(ids.first() != Some(&id)),
                Label::Loop(header) if *header == id => 0,
                Label::Dispatch(ids) if ids.contains(&id) => 0,
                _ => continue,
            };
            self.out
                .c()
                .i32_const(pending)
                .local_set(ENTRY_PENDING)
                .br(depth as u32);
            return;
        }
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
        let depth = self
            .labels
            .iter()
            .rev()
            .position(|label| matches!(label, Label::Restart))
            .unwrap();
        self.out
            .c()
            .i32_const(1)
            .local_set(ENTRY_PENDING)
            .br(depth as u32);
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
