//! Typed storage for values actually consumed by a block's executable program.
//!
//! Times are inclusive: an effect's before recovery starts at `2 * prefix`,
//! and stays live through successful definitions at `2 * prefix + 1`. Distinct
//! blocks reuse storage only because edge arguments are staged before target
//! parameters are assigned. Interned values shared by blocks retain one local.
use crate::cpu::core::region::{
    BlockId,
    values::{
        ValueType,
        executable::{
            ExecValueId, ExecutableProgram, Expr32, FlagsOrigin, Recovery, ValueNode,
            fp::{FpObligation, FpValueNode},
        },
    },
};
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::ValType;

#[cfg(test)]
mod tests;

pub(in crate::cpu::core::emitter) struct Layout {
    pub values: Vec<Option<u32>>,
    pub flags_entry: Vec<Option<u32>>,
    pub flags_effect: Vec<Option<u32>>,
    pub declarations: Vec<(u32, ValType)>,
    pub next: u32,
    live: Vec<Vec<Membership>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Interval {
    first: u32,
    last: u32,
}

impl Interval {
    fn include(&mut self, other: Self) {
        self.first = self.first.min(other.first);
        self.last = self.last.max(other.last);
    }
}

#[derive(Clone, Copy, Debug)]
struct Membership {
    block: usize,
    interval: Interval,
}

#[derive(Default)]
struct Color {
    blocks: BTreeMap<usize, Vec<Interval>>,
}

impl Color {
    fn accepts(&self, members: &[Membership]) -> bool {
        members.iter().all(|member| {
            let Some(intervals) = self.blocks.get(&member.block) else {
                return true;
            };
            let index = intervals.partition_point(|old| old.first < member.interval.first);
            (index == 0 || intervals[index - 1].last < member.interval.first)
                && (index == intervals.len() || member.interval.last < intervals[index].first)
        })
    }

    fn insert(&mut self, members: &[Membership]) {
        debug_assert!(self.accepts(members));
        for member in members {
            let intervals = self.blocks.entry(member.block).or_default();
            let index = intervals.partition_point(|old| old.first < member.interval.first);
            intervals.insert(index, member.interval);
        }
    }
}

#[derive(Default)]
struct Colors(Vec<Color>);

impl Colors {
    fn allocate(&mut self, members: &[Membership]) -> usize {
        let index = self
            .0
            .iter()
            .position(|color| color.accepts(members))
            .unwrap_or_else(|| {
                self.0.push(Color::default());
                self.0.len() - 1
            });
        self.0[index].insert(members);
        index
    }
}

fn kind(ty: ValueType) -> usize {
    match ty {
        ValueType::I32 => 0,
        ValueType::I64 => 1,
        ValueType::F64 => 2,
    }
}

fn add(intervals: &mut BTreeMap<usize, Interval>, id: usize, interval: Interval) {
    intervals
        .entry(id)
        .and_modify(|old| old.include(interval))
        .or_insert(interval);
}

fn use_values(
    program: &ExecutableProgram,
    intervals: &mut BTreeMap<usize, Interval>,
    roots: impl IntoIterator<Item = ExecValueId>,
    time: u32,
    numeric: bool,
) {
    for id in roots {
        if numeric || matches!(program.value(id).node, ValueNode::Scalar(_)) {
            add(
                intervals,
                id.0,
                Interval {
                    first: time,
                    last: time,
                },
            );
        }
    }
}

fn flag_index(origin: FlagsOrigin, blocks: usize) -> usize {
    match origin {
        FlagsOrigin::Entry(block) => block.0,
        FlagsOrigin::Effect(effect) => blocks + effect.0,
    }
}

fn use_recovery(
    program: &ExecutableProgram,
    values: &mut BTreeMap<usize, Interval>,
    flags: &mut BTreeMap<usize, Interval>,
    recovery: &Recovery,
    time: u32,
    numeric: bool,
    blocks: usize,
) {
    use_values(program, values, recovery.scalar_roots(), time, numeric);
    if numeric {
        use_values(program, values, recovery.fp.roots(), time, true);
    }
    let first = match recovery.flags.origin {
        FlagsOrigin::Entry(_) => 0,
        FlagsOrigin::Effect(effect) => program.effect(effect).point.prefix * 2 + 1,
    };
    assert!(
        first <= time,
        "flags cannot be read before their defining effect"
    );
    add(
        flags,
        flag_index(recovery.flags.origin, blocks),
        Interval { first, last: time },
    );
}

fn dependencies(program: &ExecutableProgram, id: ExecValueId) -> Vec<ExecValueId> {
    match &program.value(id).node {
        ValueNode::Scalar(Expr32::Binary { left, right, .. }) => vec![*left, *right],
        ValueNode::Scalar(Expr32::ShiftLeft { value, .. }) => vec![*value],
        ValueNode::Scalar(_) => Vec::new(),
        ValueNode::Fp(value) => value.dependencies(),
    }
}

fn expand(program: &ExecutableProgram, block: BlockId, intervals: &mut BTreeMap<usize, Interval>) {
    // Values are topologically numbered. Descending traversal propagates each
    // final interval to dependencies once, without rescanning every old value
    // at every recovery or at every unrelated block.
    let mut pending: BTreeSet<_> = intervals.keys().copied().collect();
    while let Some(index) = pending.pop_last() {
        let id = ExecValueId(index);
        let interval = intervals.get_mut(&index).unwrap();
        match &program.value(id).node {
            ValueNode::Scalar(Expr32::Input { .. }) => interval.first = 0,
            ValueNode::Fp(FpValueNode::Input { point, .. }) => {
                assert_eq!(point.block, block, "FP inputs are block-local parameters");
                // Only prefix-zero inputs belong to external ingress. Typed
                // effects also create immutable selector inputs at their own
                // prefix; keeping every such value from zero prevents their
                // storage from being reused. A canonical barrier's fresh
                // inputs name the following prefix, but its successful after
                // recovery can already use them at the preceding odd time.
                interval.first = (point.prefix * 2).min(interval.first);
            }
            ValueNode::Scalar(Expr32::EffectResult { effect, .. })
            | ValueNode::Fp(FpValueNode::EffectResult { effect, .. }) => {
                let definition = program.effect(*effect).point;
                assert_eq!(
                    definition.block, block,
                    "effect results are block-local parameters"
                );
                let time = definition.prefix * 2 + 1;
                assert!(
                    time <= interval.first,
                    "value used before its defining effect"
                );
                interval.first = time;
            }
            _ => {}
        }
        let interval = *interval;
        for dependency in dependencies(program, id) {
            assert!(
                dependency.0 < index,
                "executable dependencies are topological"
            );
            add(intervals, dependency.0, interval);
            pending.insert(dependency.0);
        }
    }
}

fn memberships(
    program: &ExecutableProgram,
    blocks: usize,
    numeric: bool,
) -> (Vec<Vec<Membership>>, Vec<Vec<Membership>>) {
    let effects = (0..blocks)
        .flat_map(|block| program.block(BlockId(block)).effects())
        .map(|effect| effect.id.0 + 1)
        .max()
        .unwrap_or(0);
    let mut values = vec![Vec::new(); program.value_count()];
    let mut flags = vec![Vec::new(); blocks + effects];
    for index in 0..blocks {
        let block = BlockId(index);
        let body = program.block(block);
        let mut local_values = BTreeMap::new();
        let mut local_flags = BTreeMap::new();
        use_values(program, &mut local_values, body.entry.gpr, 0, numeric);
        add(&mut local_flags, index, Interval { first: 0, last: 0 });
        if numeric {
            use_values(
                program,
                &mut local_values,
                program.fp_parameters(block).roots(&body.entry.fp),
                0,
                true,
            );
            for obligation in &body.entry.fp.ingress {
                match *obligation {
                    FpObligation::Native { input, value, .. } => {
                        use_values(
                            program,
                            &mut local_values,
                            input.roots().into_iter().chain([value]),
                            0,
                            true,
                        );
                    }
                    FpObligation::Occupied { tag, .. } | FpObligation::Empty { tag, .. } => {
                        use_values(program, &mut local_values, [tag], 0, true);
                    }
                }
            }
        }
        for effect in body.effects() {
            let before = effect.point.prefix * 2;
            let after = before + 1;
            use_recovery(
                program,
                &mut local_values,
                &mut local_flags,
                &effect.before,
                before,
                numeric,
                blocks,
            );
            use_values(program, &mut local_values, effect.inputs(), before, numeric);
            // A consumer can define one output before reading another old
            // input, then still fail into the before recovery. Without a
            // consumer micro-schedule all old values survive its definitions.
            use_recovery(
                program,
                &mut local_values,
                &mut local_flags,
                &effect.before,
                after,
                numeric,
                blocks,
            );
            use_values(program, &mut local_values, effect.inputs(), after, numeric);
            // Consumers define every architectural effect result, including
            // dead status/payload results. Their stores still require a local.
            use_values(
                program,
                &mut local_values,
                effect.definitions(),
                after,
                numeric,
            );
            use_recovery(
                program,
                &mut local_values,
                &mut local_flags,
                &effect.after,
                after,
                numeric,
                blocks,
            );
        }
        use_recovery(
            program,
            &mut local_values,
            &mut local_flags,
            &body.exit,
            body.exit.prefix * 2 + 1,
            numeric,
            blocks,
        );
        expand(program, block, &mut local_values);
        for (id, interval) in local_values {
            values[id].push(Membership {
                block: index,
                interval,
            });
        }
        for (id, interval) in local_flags {
            flags[id].push(Membership {
                block: index,
                interval,
            });
        }
    }
    (values, flags)
}

fn order(members: &[Vec<Membership>]) -> Vec<usize> {
    let mut ids: Vec<_> = (0..members.len())
        .filter(|&id| !members[id].is_empty())
        .collect();
    // Shared constants constrain more than one block: color them once first,
    // then reuse the same typed pools for each block's independent values.
    ids.sort_by_key(|&id| (std::cmp::Reverse(members[id].len()), id));
    ids
}

impl Layout {
    pub fn live_at(&self, id: ExecValueId, block: BlockId, time: u32) -> bool {
        let members = &self.live[id.0];
        members
            .binary_search_by_key(&block.0, |member| member.block)
            .ok()
            .is_some_and(|index| {
                let interval = members[index].interval;
                interval.first <= time && time <= interval.last
            })
    }

    pub fn build(program: &ExecutableProgram, blocks: usize, numeric: bool, base: u32) -> Self {
        let (values, flags) = memberships(program, blocks, numeric);
        let mut pools: [Colors; 3] = std::array::from_fn(|_| Colors::default());
        let mut colors = vec![None; values.len()];
        for id in order(&values) {
            let ty = kind(program.value(ExecValueId(id)).ty);
            colors[id] = Some((ty, pools[ty].allocate(&values[id])));
        }
        let mut declarations = Vec::new();
        let mut next = base;
        let mut bases = [base; 3];
        for (index, ty) in [ValType::I32, ValType::I64, ValType::F64]
            .into_iter()
            .enumerate()
        {
            bases[index] = next;
            let count = u32::try_from(pools[index].0.len()).expect("local count fits Wasm index");
            if count != 0 {
                declarations.push((count, ty));
            }
            next = next
                .checked_add(count)
                .expect("local index fits Wasm index");
        }
        let live = values;
        let values = colors
            .into_iter()
            .map(|color| color.map(|(ty, color)| bases[ty] + color as u32))
            .collect();
        let mut flag_pool = Colors::default();
        let mut flag_locals = vec![None; flags.len()];
        for id in order(&flags) {
            let color = u32::try_from(flag_pool.allocate(&flags[id]))
                .expect("flags local count fits Wasm index");
            flag_locals[id] = Some(next + color * 7);
        }
        let flag_count =
            u32::try_from(flag_pool.0.len()).expect("flags local count fits Wasm index") * 7;
        if flag_count != 0 {
            declarations.push((flag_count, ValType::I32));
        }
        next = next
            .checked_add(flag_count)
            .expect("local index fits Wasm index");
        let flags_effect = flag_locals.split_off(blocks);
        Self {
            values,
            flags_entry: flag_locals,
            flags_effect,
            declarations,
            next,
            live,
        }
    }
}
