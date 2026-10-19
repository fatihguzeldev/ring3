use super::{X87_WIDE_GROUP_MAX_STEPS, X87_WIDE_GROUP_MIN_STEPS, X87WideGroupPlan32, step};
use crate::cpu::x86::decode::DecodedOperation;

/// the legacy catalog remains intact; this additive plan selects only wider runs.
pub(in crate::cpu::dbt) fn x87_wide_groups(
    instructions: &[DecodedOperation],
) -> impl Iterator<Item = X87WideGroupPlan32> + '_ {
    let mut first = 0;
    std::iter::from_fn(move || {
        while first < instructions.len() {
            if step(&instructions[first], false).is_none() {
                first += 1;
                continue;
            }
            let mut length = 1;
            while length < X87_WIDE_GROUP_MAX_STEPS && first + length < instructions.len() {
                let previous = &instructions[first + length - 1];
                let instruction = &instructions[first + length];
                if previous.pc().0.checked_add(u32::from(previous.length()))
                    != Some(instruction.pc().0)
                {
                    break;
                }
                let Some(descriptor) = step(instruction, true) else {
                    break;
                };
                length += 1;
                if descriptor.operation.write_size() != 0 {
                    break;
                }
            }
            if length >= X87_WIDE_GROUP_MIN_STEPS {
                let plan = X87WideGroupPlan32 { first, length };
                first += length;
                return Some(plan);
            }
            first += length;
        }
        None
    })
}
