use crate::{
    cpu::x86::ir::Operation,
    memory::{CodeSnapshot, GuestAddress},
};

#[derive(Debug)]
pub(crate) struct DecodedOperation {
    pc: GuestAddress,
    length: u8,
    next_pc: GuestAddress,
    operation: Operation,
    segment_accesses: u8,
}

impl DecodedOperation {
    pub(super) fn new(
        pc: GuestAddress,
        length: u8,
        next_pc: GuestAddress,
        operation: Operation,
        segment_accesses: u8,
    ) -> Self {
        Self { pc, length, next_pc, operation, segment_accesses }
    }

    pub(crate) fn pc(&self) -> GuestAddress { self.pc }
    pub(crate) fn length(&self) -> u8 { self.length }
    pub(crate) fn next_pc(&self) -> GuestAddress { self.next_pc }
    pub(crate) fn operation(&self) -> &Operation { &self.operation }
    pub(crate) fn uses_segments(&self) -> bool { self.segment_accesses != 0 }
    pub(crate) fn segment_accesses(&self) -> u8 { self.segment_accesses }
}

#[derive(Debug)]
pub struct DecodedInstruction {
    instruction: DecodedOperation,
    code_snapshot: CodeSnapshot,
}

impl DecodedInstruction {
    pub(super) fn new(instruction: DecodedOperation, code_snapshot: CodeSnapshot) -> Self {
        Self { instruction, code_snapshot }
    }

    pub fn pc(&self) -> GuestAddress { self.instruction.pc() }
    pub fn length(&self) -> u8 { self.instruction.length() }
    pub fn next_pc(&self) -> GuestAddress { self.instruction.next_pc() }
    pub fn operation(&self) -> &Operation { self.instruction.operation() }
    pub fn code_snapshot(&self) -> &CodeSnapshot { &self.code_snapshot }

    #[cfg(test)]
    pub(crate) fn uses_segments(&self) -> bool { self.instruction.uses_segments() }
    #[cfg(test)]
    pub(crate) fn segment_accesses(&self) -> u8 { self.instruction.segment_accesses() }
}
