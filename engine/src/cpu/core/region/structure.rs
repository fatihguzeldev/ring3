use std::collections::BTreeSet;

use super::{BlockId, Region};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum Shape {
    Block(BlockId),
    Loop {
        header: BlockId,
        body: Vec<Shape>,
    },
    Dispatch {
        entries: Vec<BlockId>,
        blocks: Vec<BlockId>,
    },
}

pub(in crate::cpu::core) struct StructurePlan {
    pub shapes: Vec<Shape>,
    pub reachable: Vec<bool>,
}

impl StructurePlan {
    pub fn requires_dispatch(&self) -> bool {
        fn contains(shapes: &[Shape]) -> bool {
            shapes.iter().any(|shape| match shape {
                Shape::Block(_) => false,
                Shape::Loop { body, .. } => contains(body),
                Shape::Dispatch { .. } => true,
            })
        }
        contains(&self.shapes)
    }
}

pub(super) fn plan(region: &Region, entry: BlockId) -> StructurePlan {
    let mut reachable = vec![false; region.blocks.len()];
    let mut pending = vec![entry];
    while let Some(id) = pending.pop() {
        if !reachable[id.0] {
            reachable[id.0] = true;
            pending.extend(region.blocks[id.0].successors.iter().copied());
        }
    }
    let members: Vec<_> = reachable
        .iter()
        .enumerate()
        .filter_map(|(id, &reached)| reached.then_some(BlockId(id)))
        .collect();
    StructurePlan {
        shapes: sequence(region, &members, &reachable, entry),
        reachable,
    }
}

pub(super) fn plan_all(region: &Region) -> StructurePlan {
    let members: Vec<_> = (0..region.blocks.len()).map(BlockId).collect();
    let reachable = vec![true; region.blocks.len()];
    StructurePlan {
        shapes: sequence(region, &members, &reachable, region.entry),
        reachable,
    }
}

fn sequence(
    region: &Region,
    members: &[BlockId],
    reachable: &[bool],
    entry: BlockId,
) -> Vec<Shape> {
    let components = components(region, members);
    let mut membership = vec![usize::MAX; region.blocks.len()];
    for (index, component) in components.iter().enumerate() {
        for id in component {
            membership[id.0] = index;
        }
    }
    let mut edges = vec![BTreeSet::new(); components.len()];
    let mut incoming = vec![0; components.len()];
    for (index, component) in components.iter().enumerate() {
        for id in component {
            for successor in &region.blocks[id.0].successors {
                let target = membership[successor.0];
                if target != usize::MAX && target != index && edges[index].insert(target) {
                    incoming[target] += 1;
                }
            }
        }
    }
    let mut ready = BTreeSet::new();
    for (index, component) in components.iter().enumerate() {
        if incoming[index] == 0 {
            ready.insert((component[0], index));
        }
    }
    let mut shapes = Vec::with_capacity(components.len());
    while let Some((_, index)) = ready.pop_first() {
        let component = &components[index];
        let cyclic = component.len() > 1
            || region.blocks[component[0].0]
                .successors
                .contains(&component[0]);
        if !cyclic {
            shapes.push(Shape::Block(component[0]));
        } else {
            let mut entries: Vec<_> = component
                .iter()
                .copied()
                .filter(|id| {
                    *id == entry
                        || region.blocks[id.0]
                            .predecessors
                            .iter()
                            .any(|pred| reachable[pred.0] && membership[pred.0] != index)
                })
                .collect();
            if entries.is_empty() {
                // a separately advertised entry may own a disconnected cycle.
                entries.push(component[0]);
            }
            if let [header] = entries.as_slice() {
                let remaining: Vec<_> = component
                    .iter()
                    .copied()
                    .filter(|id| id != header)
                    .collect();
                let mut body = vec![Shape::Block(*header)];
                body.extend(sequence(region, &remaining, reachable, entry));
                shapes.push(Shape::Loop {
                    header: *header,
                    body,
                });
            } else {
                shapes.push(Shape::Dispatch {
                    entries,
                    blocks: component.clone(),
                });
            }
        }
        for &target in &edges[index] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                ready.insert((components[target][0], target));
            }
        }
    }
    shapes
}

fn components(region: &Region, members: &[BlockId]) -> Vec<Vec<BlockId>> {
    struct Search<'a> {
        region: &'a Region,
        active: Vec<bool>,
        indices: Vec<usize>,
        low: Vec<usize>,
        next: usize,
        stack: Vec<BlockId>,
        stacked: Vec<bool>,
        components: Vec<Vec<BlockId>>,
    }
    impl Search<'_> {
        fn visit(&mut self, id: BlockId) {
            self.indices[id.0] = self.next;
            self.low[id.0] = self.next;
            self.next += 1;
            self.stack.push(id);
            self.stacked[id.0] = true;
            for successor in self.region.blocks[id.0].successors.clone() {
                if !self.active[successor.0] {
                    continue;
                }
                if self.indices[successor.0] == usize::MAX {
                    self.visit(successor);
                    self.low[id.0] = self.low[id.0].min(self.low[successor.0]);
                } else if self.stacked[successor.0] {
                    self.low[id.0] = self.low[id.0].min(self.indices[successor.0]);
                }
            }
            if self.low[id.0] == self.indices[id.0] {
                let mut component = Vec::new();
                loop {
                    let member = self
                        .stack
                        .pop()
                        .expect("active strongly connected component");
                    self.stacked[member.0] = false;
                    component.push(member);
                    if member == id {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }
    let count = region.blocks.len();
    let mut search = Search {
        region,
        active: vec![false; count],
        indices: vec![usize::MAX; count],
        low: vec![0; count],
        next: 0,
        stack: Vec::new(),
        stacked: vec![false; count],
        components: Vec::new(),
    };
    for id in members {
        search.active[id.0] = true;
    }
    for &id in members {
        if search.indices[id.0] == usize::MAX {
            search.visit(id);
        }
    }
    search.components
}
