//! Executable spans are distinct from immutable source/coverage blocks.
//! Splitting a span never creates another source owner or a public window alias.
use super::Block;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum ExecutableWork {
    Fixed(u32),
    /// the exact kernel consumes positive fuel and owns partial progress.
    Budgeted,
}

impl ExecutableWork {
    pub const fn minimum_work(self) -> u32 {
        match self {
            Self::Fixed(work) => work,
            Self::Budgeted => 1,
        }
    }
}

pub(in crate::cpu::core) struct ExecutableSpan {
    pub block: Block,
    pub trace_entry: u32,
    pub trace_end: u32,
    pub trace_offset: u32,
    work: ExecutableWork,
}

impl ExecutableSpan {
    pub const fn work(&self) -> ExecutableWork {
        self.work
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::core::program::Instruction;
    use crate::cpu::x86::{ControlOperation, ir::Operation};

    #[test]
    fn terminal_admission_does_not_force_a_completed_prefix_into_precise_execution() {
        for (operation, gate) in [
            (
                Some(Operation::Control(ControlOperation::InvalidOpcode)),
                None,
            ),
            (Some(Operation::Control(ControlOperation::Breakpoint)), None),
            (Some(Operation::Control(ControlOperation::Sysenter)), None),
            (None, None),
            (None, Some(7)),
        ] {
            let source = Block {
                entry: 0x1000,
                instructions: vec![
                    Instruction {
                        pc: 0x1000,
                        next: 0x1001,
                        operation: Some(Operation::Nop),
                        gate: None,
                    },
                    Instruction {
                        pc: 0x1001,
                        next: 0x1003,
                        operation,
                        gate,
                    },
                ],
            };
            let spans = partition(&[source]);
            assert_eq!(spans.len(), 2);
            assert_eq!(spans[0].block.entry, 0x1000);
            assert_eq!(spans[1].block.entry, 0x1001);
            assert!(
                spans
                    .iter()
                    .all(|span| span.work() == ExecutableWork::Fixed(1))
            );
            assert!(
                spans
                    .iter()
                    .all(|span| span.trace_entry == 0x1000 && span.trace_end == 0x1003)
            );
            assert_eq!(spans[0].trace_offset, 0);
            assert_eq!(spans[1].trace_offset, 1);
        }
    }

    #[test]
    fn self_accounting_strings_are_isolated_with_fixed_or_budgeted_work() {
        use crate::cpu::x86::ir::{AddressSize, RepeatCondition, StringKind, StringWidth};
        for repeated in [false, true] {
            let operation = if repeated {
                Operation::RepeatString {
                    kind: StringKind::Move,
                    width: StringWidth::Byte,
                    address_size: AddressSize::Bits32,
                    source_segment: None,
                    repeat: RepeatCondition::Always,
                }
            } else {
                Operation::String {
                    kind: StringKind::Move,
                    width: StringWidth::Byte,
                    address_size: AddressSize::Bits32,
                    source_segment: None,
                }
            };
            let instructions = vec![
                Instruction {
                    pc: 0x1000,
                    next: 0x1001,
                    operation: Some(Operation::Nop),
                    gate: None,
                },
                Instruction {
                    pc: 0x1001,
                    next: 0x1003,
                    operation: Some(operation),
                    gate: None,
                },
                Instruction {
                    pc: 0x1003,
                    next: 0x1004,
                    operation: Some(Operation::Nop),
                    gate: None,
                },
            ];
            let spans = partition(&[Block {
                entry: 0x1000,
                instructions,
            }]);
            assert_eq!(spans.len(), 3);
            assert_eq!(spans[0].work(), ExecutableWork::Fixed(1));
            assert_eq!(
                spans[1].work(),
                if repeated {
                    ExecutableWork::Budgeted
                } else {
                    ExecutableWork::Fixed(1)
                }
            );
            assert_eq!(spans[2].work(), ExecutableWork::Fixed(1));
            assert!(spans.iter().all(|span| span.work().minimum_work() == 1));
            assert_eq!(
                spans
                    .iter()
                    .map(|span| span.trace_offset)
                    .collect::<Vec<_>>(),
                [0, 1, 2]
            );
        }
    }
}

pub(in crate::cpu::core) fn partition(blocks: &[Block]) -> Vec<ExecutableSpan> {
    let mut spans = Vec::new();
    for block in blocks {
        let Some(last) = block.instructions.last() else {
            continue;
        };
        let mut first = 0;
        for (index, instruction) in block.instructions.iter().enumerate() {
            // An explicit terminal is its own admission boundary. A quota
            // ending immediately before UD2/host/debug must still execute the
            // completed ordinary prefix without publishing its private state.
            let terminal = instruction.gate.is_some()
                || matches!(
                    instruction.operation,
                    None | Some(crate::cpu::x86::ir::Operation::Control(
                        crate::cpu::x86::ControlOperation::InvalidOpcode
                            | crate::cpu::x86::ControlOperation::Breakpoint
                            | crate::cpu::x86::ControlOperation::Sysenter
                    ))
                );
            let variable = instruction
                .operation
                .is_some_and(super::super::emitter::variable_work);
            let string = matches!(
                instruction.operation,
                Some(
                    crate::cpu::x86::ir::Operation::String { .. }
                        | crate::cpu::x86::ir::Operation::RepeatString { .. }
                )
            );
            // both string forms own their accounting. ordinary String has fixed
            // one-work semantics but must not absorb a scalar prefix or suffix.
            if (string || terminal) && first != index {
                push(&mut spans, block, first, index, last.next, true);
                first = index;
            }
            let canonical = instruction.operation.is_some_and(|operation| {
                super::super::emitter::is_native(&operation)
                    && !super::super::region::values::inline_numeric_candidate(operation)
            });
            if string || canonical {
                push(&mut spans, block, first, index + 1, last.next, !variable);
                first = index + 1;
            }
        }
        if first != block.instructions.len() {
            push(
                &mut spans,
                block,
                first,
                block.instructions.len(),
                last.next,
                true,
            );
        }
    }
    spans
}

fn push(
    spans: &mut Vec<ExecutableSpan>,
    source: &Block,
    first: usize,
    end: usize,
    trace_end: u32,
    fixed_work: bool,
) {
    let instructions = source.instructions[first..end].to_vec();
    spans.push(ExecutableSpan {
        block: Block {
            entry: instructions[0].pc,
            instructions,
        },
        trace_entry: source.entry,
        trace_end,
        trace_offset: first as u32,
        work: if fixed_work {
            ExecutableWork::Fixed((end - first) as u32)
        } else {
            ExecutableWork::Budgeted
        },
    });
}
