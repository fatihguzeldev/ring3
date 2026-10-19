use super::{X87PairStep32, step};
use crate::cpu::x86::decode::DecodedOperation;

mod plan;
pub(in crate::cpu::dbt) use plan::x87_wide_groups;

pub(crate) const X87_WIDE_GROUP_MIN_STEPS: usize = 9;
pub(crate) const X87_WIDE_GROUP_MAX_STEPS: usize = 32;
pub(crate) const X87_WIDE_GROUP_COMPLETED_SHIFT: u32 = 32;
pub(crate) const X87_WIDE_GROUP_REPORTED_SHIFT: u32 = 38;

/// an immutable final-ir view; native request validation precedes fp decoding.
#[derive(Clone, Copy)]
pub(crate) struct X87WideGroup32<'a> {
    instructions: &'a [DecodedOperation],
}

impl X87WideGroup32<'_> {
    pub(crate) fn len(self) -> usize {
        self.instructions.len()
    }

    pub(crate) fn descriptor(self, index: usize) -> Option<X87PairStep32> {
        let instruction = self.instructions.get(index)?;
        if index != 0 {
            let previous = &self.instructions[index - 1];
            if previous.pc().0.checked_add(u32::from(previous.length())) != Some(instruction.pc().0)
            {
                return None;
            }
        }
        step(instruction, index + 1 == self.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::dbt) struct X87WideGroupPlan32 {
    pub(in crate::cpu::dbt) first: usize,
    pub(in crate::cpu::dbt) length: usize,
}

impl X87WideGroupPlan32 {
    pub(in crate::cpu::dbt) fn view(
        self,
        instructions: &[DecodedOperation],
    ) -> Option<X87WideGroup32<'_>> {
        if !(X87_WIDE_GROUP_MIN_STEPS..=X87_WIDE_GROUP_MAX_STEPS).contains(&self.length) {
            return None;
        }
        let end = self.first.checked_add(self.length)?;
        Some(X87WideGroup32 {
            instructions: instructions.get(self.first..end)?,
        })
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) struct X87WideGroupProgress32;

#[cfg(any(test, target_arch = "wasm32"))]
impl X87WideGroupProgress32 {
    pub(crate) const fn encode(status: u32, completed: u32, reported_step: u32) -> u64 {
        debug_assert!(completed <= X87_WIDE_GROUP_MAX_STEPS as u32);
        debug_assert!(reported_step < X87_WIDE_GROUP_MAX_STEPS as u32);
        debug_assert!(completed >= reported_step && completed <= reported_step + 1);
        status as u64
            | ((completed as u64) << X87_WIDE_GROUP_COMPLETED_SHIFT)
            | ((reported_step as u64) << X87_WIDE_GROUP_REPORTED_SHIFT)
    }
}

#[cfg(test)]
mod tests;
