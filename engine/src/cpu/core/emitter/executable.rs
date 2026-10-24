//! Lower only executable scalar roots; prefix maps remain recovery metadata.
use super::{CARRY_IN, KIND, LEFT, Output, RIGHT, VALUE, WIDTH};
use crate::cpu::core::region::values::executable::{Binary32, Expr32, FlagsRecipe, PureRun};
use std::collections::BTreeMap;

impl Output<'_> {
    pub(super) fn pure_run(&mut self, run: &PureRun) {
        let live = self.executable.live_expressions(run).to_vec();
        let slots: BTreeMap<_, _> = live
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, self.expression_base + index as u32))
            .collect();
        for &id in &live {
            match *self.executable.expression(id) {
                Expr32::Input { register, .. } => {
                    self.c().local_get(u32::from(register) + 2);
                }
                Expr32::Constant(value) => {
                    self.c().i32_const(value as i32);
                }
                Expr32::Binary { kind, left, right } => {
                    self.c().local_get(slots[&left]).local_get(slots[&right]);
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
                    self.c()
                        .local_get(slots[&value])
                        .i32_const(i32::from(amount))
                        .i32_shl();
                }
            }
            self.c().local_set(slots[&id]);
        }
        // Every input/expression is captured before any architectural local is
        // overwritten, including swaps and a flag writer's old destination.
        for register in 0..8 {
            if run.writes & (1 << register) != 0 {
                self.c()
                    .local_get(slots[&run.after.gpr[register]])
                    .local_set(register as u32 + 2);
            }
        }
        if run.flags_changed() {
            if let FlagsRecipe::Arithmetic {
                kind,
                width,
                left,
                right,
                result,
            } = run.after.flags
            {
                self.c()
                    .i32_const(kind as i32)
                    .local_set(KIND)
                    .i32_const(width as i32)
                    .local_set(WIDTH)
                    .local_get(slots[&left])
                    .local_set(LEFT)
                    .local_get(slots[&right])
                    .local_set(RIGHT)
                    .local_get(slots[&result])
                    .local_set(VALUE)
                    .i32_const(0)
                    .local_set(CARRY_IN);
            }
        }
        if run.writes != 0 {
            self.address_fact = None;
        }
        self.before_frame = Some(run.before.frame);
        self.after_frame = Some(run.after.frame);
        self.frame = self.after_frame;
        self.prefix = run.end_prefix;
    }
}
