use crate::{
    cpu::{
        core::{
            Context32, StopReason,
            cold::{self, ColdState, ColdOutcome},
        },
        x86::SegmentContext,
    },
    memory::GuestAddress,
    process::EngineInstance,
};

#[cfg(target_arch = "wasm32")]
use super::super::numeric::NumericBank;
#[cfg(not(target_arch = "wasm32"))]
use crate::process::core_numeric_tests::NumericBank;

pub(in crate::process) fn run(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    heat: &mut ColdState,
) -> StopReason {
    loop {
        let outcome = {
            let Some(memory) = instance.memory.as_mut() else {
                return StopReason::Infrastructure;
            };
            cold::run(memory, context, segments, gates, frontier, heat)
        };
        let instruction = match outcome {
            ColdOutcome::Stop(reason) => return reason,
            ColdOutcome::NeedNative(instruction) => instruction,
        };
        if context.remaining() == 0 || context.state().eip != instruction.pc {
            return StopReason::Infrastructure;
        }
        let result = numeric.execute(instance, context, &instruction);
        if !matches!(result, 0 | 6 | 10) {
            return match result {
                1 => StopReason::NeedCode,
                2 => StopReason::Budget,
                3 => StopReason::MemoryFault,
                4 => StopReason::Unsupported,
                5 => StopReason::Debug,
                7 => StopReason::Host,
                9 => StopReason::CpuFault,
                _ => StopReason::Infrastructure,
            };
        }
        context.set_word(32, instruction.next);
        context.set_word(40, context.retired().wrapping_add(1));
        context.set_word(44, context.remaining() - 1);
        let Some(memory) = instance.memory.as_ref() else {
            return StopReason::Infrastructure;
        };
        if result != 6 {
            heat.retired(memory, instruction.pc);
        }
        match result {
            6 => return StopReason::CodeInvalidated,
            10 => return StopReason::SystemEntry,
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
