use super::X87PairStep32;
use crate::cpu::x86::decode::DecodedOperation;

mod wide;
#[cfg(target_arch = "wasm32")]
pub(crate) use wide::X87WideGroupProgress32;
pub(crate) use wide::{
    X87_WIDE_GROUP_COMPLETED_SHIFT, X87_WIDE_GROUP_MAX_STEPS, X87_WIDE_GROUP_REPORTED_SHIFT,
    X87WideGroup32,
};
pub(super) use wide::{X87WideGroupPlan32, x87_wide_groups};

pub(crate) const X87_GROUP_MIN_STEPS: usize = 4;
pub(crate) const X87_GROUP_MAX_STEPS: usize = 8;
pub(crate) const X87_GROUP_COMPLETED_SHIFT: u32 = 32;
pub(crate) const X87_GROUP_REPORTED_SHIFT: u32 = 36;

fn step(instruction: &DecodedOperation, final_step: bool) -> Option<X87PairStep32> {
    instruction
        .pc()
        .0
        .checked_add(u32::from(instruction.length()))?;
    X87PairStep32::from_instruction(instruction, final_step)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct X87Group32 {
    steps: [X87PairStep32; X87_GROUP_MAX_STEPS],
    length: u8,
}

impl X87Group32 {
    pub(crate) fn steps(&self) -> &[X87PairStep32] {
        &self.steps[..self.len()]
    }

    pub(crate) const fn len(&self) -> usize {
        self.length as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct X87GroupPlan32 {
    pub(super) first: usize,
    pub(super) length: usize,
}

impl X87GroupPlan32 {
    pub(super) fn descriptors(self, instructions: &[DecodedOperation]) -> Option<X87Group32> {
        if !(X87_GROUP_MIN_STEPS..=X87_GROUP_MAX_STEPS).contains(&self.length) {
            return None;
        }
        let selected = instructions.get(self.first..self.first.checked_add(self.length)?)?;
        let first = step(selected.first()?, false)?;
        let mut group = X87Group32 {
            steps: [first; X87_GROUP_MAX_STEPS],
            length: self.length as u8,
        };
        for (index, instruction) in selected.iter().enumerate() {
            if index != 0
                && group.steps[index - 1]
                    .pc
                    .checked_add(u32::from(group.steps[index - 1].length))
                    != Some(instruction.pc().0)
            {
                return None;
            }
            group.steps[index] = step(instruction, index + 1 == self.length)?;
        }
        Some(group)
    }
}

/// choose bounded runs while retaining every ordinary interior instruction body.
pub(super) fn x87_groups(
    instructions: &[DecodedOperation],
) -> impl Iterator<Item = X87GroupPlan32> + '_ {
    let mut first = 0;
    std::iter::from_fn(move || {
        while first < instructions.len() {
            if step(&instructions[first], false).is_none() {
                first += 1;
                continue;
            }
            let mut length = 1;
            while length < X87_GROUP_MAX_STEPS && first + length < instructions.len() {
                let previous = &instructions[first + length - 1];
                let instruction = &instructions[first + length];
                if previous.pc().0.checked_add(u32::from(previous.length()))
                    != Some(instruction.pc().0)
                {
                    break;
                }
                let Some(step) = step(instruction, true) else {
                    break;
                };
                length += 1;
                if step.operation.write_size() != 0 {
                    break;
                }
            }
            if length >= X87_GROUP_MIN_STEPS {
                let selected = X87GroupPlan32 { first, length };
                first += length;
                return Some(selected);
            }
            first += length;
        }
        None
    })
}

pub(crate) struct X87GroupProgress32;

impl X87GroupProgress32 {
    pub(crate) const fn encode(status: u32, completed: u32, reported_step: u32) -> u64 {
        debug_assert!(completed <= X87_GROUP_MAX_STEPS as u32);
        debug_assert!(reported_step < X87_GROUP_MAX_STEPS as u32);
        debug_assert!(completed >= reported_step && completed <= reported_step + 1);
        status as u64
            | ((completed as u64) << X87_GROUP_COMPLETED_SHIFT)
            | ((reported_step as u64) << X87_GROUP_REPORTED_SHIFT)
    }
}

#[cfg(test)]
mod tests;
