use crate::cpu::x86::{
    X87Format, X87Operation,
    decode::DecodedOperation,
    ir::{EffectiveAddress, Operation},
};

pub(crate) const X87_PAIR_COMPLETED_SHIFT: u32 = 32;
pub(crate) const X87_PAIR_REPORTED_SHIFT: u32 = 34;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct X87PairStep32 {
    pub(crate) operation: X87Operation,
    pub(crate) address: Option<EffectiveAddress>,
    pub(crate) pc: u32,
    pub(crate) next_pc: u32,
    pub(crate) length: u8,
    pub(crate) opcode: u16,
    pub(crate) control: u32,
}

impl X87PairStep32 {
    fn from_instruction(instruction: &DecodedOperation, final_step: bool) -> Option<Self> {
        let Operation::X87 {
            operation,
            address,
            waiting,
            opcode,
        } = *instruction.operation()
        else {
            return None;
        };
        let computation = operation.is_computation()
            && operation.write_size() == 0
            && !matches!(
                operation,
                X87Operation::Compare {
                    integer_flags: true,
                    ..
                }
            );
        let final_store = final_step
            && address.is_some()
            && matches!(
                operation,
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false,
                }
            );
        if !computation && !final_store {
            return None;
        }
        let segment = address.map_or(0, |address| {
            (address.resolved_segment().index() as u32 + 1) << 8
        });
        Some(Self {
            operation,
            address,
            pc: instruction.pc().0,
            next_pc: instruction.next_pc().0,
            length: instruction.length(),
            opcode,
            control: segment | u32::from(waiting),
        })
    }
}

/// choose nonoverlapping pairs without changing the retained instruction entries.
pub(crate) fn x87_pairs(
    instructions: &[DecodedOperation],
) -> impl Iterator<Item = [X87PairStep32; 2]> + '_ {
    let mut index = 0;
    std::iter::from_fn(move || {
        while index + 1 < instructions.len() {
            let first = &instructions[index];
            let second = &instructions[index + 1];
            if first.pc().0.checked_add(u32::from(first.length())) == Some(second.pc().0)
                && let Some(first) = X87PairStep32::from_instruction(first, false)
                && let Some(second) = X87PairStep32::from_instruction(second, true)
            {
                index += 2;
                return Some([first, second]);
            }
            index += 1;
        }
        None
    })
}

pub(crate) struct X87PairProgress32;

impl X87PairProgress32 {
    pub(crate) const fn encode(status: u32, completed: u32, reported_step: u32) -> u64 {
        debug_assert!(completed <= 2);
        debug_assert!(reported_step <= 1);
        debug_assert!(completed >= reported_step && completed <= reported_step + 1);
        status as u64
            | ((completed as u64) << X87_PAIR_COMPLETED_SHIFT)
            | ((reported_step as u64) << X87_PAIR_REPORTED_SHIFT)
    }
}

#[cfg(test)]
mod tests;
