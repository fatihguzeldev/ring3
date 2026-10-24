//! Demand live mixed values; ordered effects alone force architectural work.

pub(super) mod locals;
use super::{CARRY_IN, FLAGS, KIND, LEFT, Output, RIGHT, VALUE, WIDTH};
use crate::cpu::core::region::{
    BlockId,
    values::executable::{
        AddressValue, Binary32, ExecValueId, Expr32, FlagsOrigin, FlagsRecipe, FlagsState,
        OrderedEffect, PureRun, Recovery, ValueNode,
    },
};

const FLAG_TUPLE: [u32; 7] = [FLAGS, KIND, WIDTH, LEFT, RIGHT, VALUE, CARRY_IN];

impl Output<'_> {
    fn value_local(&self, id: ExecValueId) -> u32 {
        self.local_layout.values[id.0].expect("only a consumed executable value has a local")
    }

    pub(super) fn is_defined(&self, id: ExecValueId) -> bool {
        self.available.contains(&id)
    }

    pub(super) fn define(&mut self, id: ExecValueId) {
        let local = self.value_local(id);
        self.c().local_set(local);
        self.available.insert(id);
    }

    pub(super) fn demand(&mut self, id: ExecValueId) {
        let local = self.value_local(id);
        if !self.available.contains(&id) {
            match self.executable.value(id).node.clone() {
                ValueNode::Scalar(expression) => match expression {
                    Expr32::Input { register, .. } => {
                        assert_eq!(
                            self.physical_gpr.map(|values| values[register as usize]),
                            Some(id),
                            "an architectural input must be pinned before its physical local is overwritten"
                        );
                        self.c().local_get(u32::from(register) + 2);
                    }
                    Expr32::Constant(value) => {
                        self.c().i32_const(value as i32);
                    }
                    Expr32::Binary { kind, left, right } => {
                        self.demand(left);
                        self.demand(right);
                        match kind {
                            Binary32::Add => {
                                self.c().i32_add();
                            }
                            Binary32::Sub => {
                                self.c().i32_sub();
                            }
                            Binary32::And => {
                                self.c().i32_and();
                            }
                            Binary32::Or => {
                                self.c().i32_or();
                            }
                            Binary32::Xor => {
                                self.c().i32_xor();
                            }
                        }
                    }
                    Expr32::ShiftLeft { value, amount } => {
                        self.demand(value);
                        self.c().i32_const(i32::from(amount)).i32_shl();
                    }
                    Expr32::EffectResult { effect, register } => {
                        assert_eq!(
                            Some(effect),
                            self.active_effect,
                            "an earlier effect definition must already be captured"
                        );
                        self.c().local_get(u32::from(register) + 2);
                    }
                },
                ValueNode::Fp(node) => super::numeric::executable::emit_value(self, &node),
            }
            self.define(id);
        }
        self.c().local_get(local);
    }

    fn flag_base(&self, origin: FlagsOrigin) -> u32 {
        match origin {
            FlagsOrigin::Entry(block) => self.local_layout.flags_entry[block.0],
            FlagsOrigin::Effect(effect) => self.local_layout.flags_effect[effect.0],
        }
        .expect("a consumed flags origin has one stable tuple")
    }

    fn capture_flags(&mut self, origin: FlagsOrigin) {
        let base = self.flag_base(origin);
        for (index, local) in FLAG_TUPLE.into_iter().enumerate() {
            self.c().local_get(local).local_set(base + index as u32);
        }
    }

    fn demand_flags(&mut self, state: FlagsState) {
        if self.physical_flags == Some(state) {
            return;
        }
        let base = self.flag_base(state.origin);
        for (index, local) in FLAG_TUPLE.into_iter().enumerate() {
            self.c().local_get(base + index as u32).local_set(local);
        }
        if let FlagsRecipe::Arithmetic {
            kind,
            width,
            left,
            right,
            result,
        } = state.recipe
        {
            self.demand(left);
            self.c().local_set(LEFT);
            self.demand(right);
            self.c().local_set(RIGHT);
            self.demand(result);
            self.c().local_set(VALUE);
            self.c()
                .i32_const(kind as i32)
                .local_set(KIND)
                .i32_const(width as i32)
                .local_set(WIDTH)
                .i32_const(0)
                .local_set(CARRY_IN);
        }
        self.physical_flags = Some(state);
    }

    pub(super) fn begin_executable_block(&mut self, block: BlockId) {
        self.available.clear();
        self.active_effect = None;
        self.active_fp_recovery = None;
        self.captured_effect = None;
        let entry = self.executable.block(block).entry.clone();
        self.active_recovery = Some(entry.clone());
        self.pc = match self.plan.frame(entry.frame).pc {
            crate::cpu::core::region::ResumePc::Exact(pc) => pc,
            crate::cpu::core::region::ResumePc::Computed => unreachable!("block ingress is exact"),
        };
        self.prefix = 0;
        self.physical_gpr = Some(entry.gpr);
        self.physical_flags = Some(entry.flags);
        self.capture_flags(entry.flags.origin);
    }

    pub(super) fn recover_scalar(
        &mut self,
        recovery: &Recovery,
        kernel_gpr: u8,
        kernel_flags: bool,
    ) {
        // Evaluate every changed root before overwriting any physical local;
        // parallel copies and exact branch-only recovery share this rule.
        let physical = self.physical_gpr.unwrap_or(recovery.gpr);
        let changed: Vec<_> = (0..8)
            .filter(|&r| kernel_gpr & (1 << r) == 0 && physical[r] != recovery.gpr[r])
            .collect();
        for &register in &changed {
            self.demand(recovery.gpr[register]);
            self.c().drop();
        }
        for register in changed {
            self.demand(recovery.gpr[register]);
            self.c().local_set(register as u32 + 2);
            self.physical_gpr.as_mut().unwrap()[register] = recovery.gpr[register];
        }
        if !kernel_flags {
            self.demand_flags(recovery.flags);
        }
    }

    pub(super) fn prepare_effect(&mut self, effect: &OrderedEffect) {
        self.active_effect = Some(effect.id);
        self.captured_effect = None;
        self.before_frame = Some(effect.before.frame);
        self.after_frame = Some(effect.after.frame);
        self.frame = self.before_frame;
        self.prefix = effect.point.prefix;
        self.pc = match self.plan.frame(effect.before.frame).pc {
            crate::cpu::core::region::ResumePc::Exact(pc) => pc,
            crate::cpu::core::region::ResumePc::Computed => unreachable!("effect ingress is exact"),
        };
        self.active_recovery = Some(effect.before.clone());
        let entry = self.executable.block(effect.point.block).entry.gpr;
        for register in 0..8 {
            if (effect.contract.gpr_reads | effect.contract.gpr_writes) & (1 << register) != 0
                && self.local_layout.live_at(
                    entry[register],
                    effect.point.block,
                    effect.point.prefix * 2,
                )
                && !self.is_defined(entry[register])
                && self
                    .physical_gpr
                    .is_some_and(|values| values[register] == entry[register])
            {
                self.c().local_get(register as u32 + 2);
                self.define(entry[register]);
            }
        }
        for register in 0..8 {
            if effect.contract.gpr_reads & (1 << register) != 0 {
                self.demand(effect.before.gpr[register]);
                self.c().local_set(register as u32 + 2);
                self.physical_gpr.as_mut().unwrap()[register] = effect.before.gpr[register];
            }
        }
        if effect.contract.flags_read != 0 || effect.contract.flags_write != 0 {
            self.demand_flags(effect.before.flags);
        }
        if matches!(
            effect.kind,
            crate::cpu::core::region::values::executable::EffectKind::Exact(_)
        ) {
            super::numeric::executable::prepare_exact(self, effect);
        }
    }

    pub(super) fn capture_effect(&mut self, effect: &OrderedEffect) {
        if self.captured_effect == Some(effect.id) {
            return;
        }
        if matches!(
            effect.kind,
            crate::cpu::core::region::values::executable::EffectKind::Exact(_)
        ) {
            super::numeric::executable::finish_exact(self, effect);
        }
        for register in 0..8 {
            if effect.contract.gpr_writes & (1 << register) != 0 {
                self.physical_gpr.as_mut().unwrap()[register] = effect.after.gpr[register];
                if !self.is_defined(effect.after.gpr[register]) {
                    self.c().local_get(register as u32 + 2);
                    self.define(effect.after.gpr[register]);
                }
            }
        }
        if effect.contract.flags_write != 0 {
            self.capture_flags(effect.after.flags.origin);
            self.physical_flags = Some(effect.after.flags);
        }
        self.captured_effect = Some(effect.id);
    }

    pub(super) fn finish_effect(&mut self, effect: &OrderedEffect) {
        self.capture_effect(effect);
        self.active_recovery = Some(effect.after.clone());
    }

    pub(super) fn pure_run(&mut self, run: &PureRun) {
        // No architectural flush here. Both subsequent effects and side exits
        // demand their own immutable roots from this same value program.
        self.before_frame = Some(run.before.frame);
        self.after_frame = Some(run.after.frame);
        self.frame = self.after_frame;
        self.prefix = run.end_prefix;
        self.active_recovery = Some(run.after.clone());
    }

    pub(super) fn executable_address(&mut self, address: &AddressValue) {
        use crate::cpu::x86::SegmentRegister;
        self.stack_access = None;
        self.address_fact = None;
        self.demand(address.offset);
        self.c().local_tee(super::LOGICAL_ADDRESS);
        if let Some(offset) = match address.address.resolved_segment() {
            SegmentRegister::Fs => Some(104),
            SegmentRegister::Gs => Some(108),
            _ => None,
        } {
            self.c()
                .local_get(super::CTX)
                .i32_load(super::mem(offset, 2))
                .i32_add();
        }
        self.c().local_set(super::ADDRESS);
        self.remember_executable_address(address.address);
    }

    pub(super) fn emit_plain_read(&mut self, address: &AddressValue, result: ExecValueId) {
        self.executable_address(address);
        self.read(4, super::RESULT);
        self.c().local_get(super::RESULT);
        self.define(result);
        if let Expr32::EffectResult { register, .. } = *self.executable.expression(result) {
            self.c()
                .local_get(super::RESULT)
                .local_set(u32::from(register) + 2);
            self.physical_gpr.as_mut().unwrap()[register as usize] = result;
        }
    }
}
