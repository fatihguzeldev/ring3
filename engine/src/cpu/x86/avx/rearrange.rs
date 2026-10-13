use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth};
use crate::{cpu::x86::simd::RearrangeOperation, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RearrangeInstruction {
    operation: RearrangeOperation,
    width: VectorWidth,
    destination: VectorRegister,
    first: Option<VectorRegister>,
    second: MoveSource,
    mask: Option<VectorRegister>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RearrangeError {
    OperandLength,
    InvalidState,
}

impl RearrangeInstruction {
    pub(crate) fn new(
        operation: RearrangeOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
        mask: Option<VectorRegister>,
    ) -> Option<Self> {
        (operation.unary() == first.is_none() && operation.uses_mask() == mask.is_some()).then_some(
            Self {
                operation,
                width: VectorWidth::Xmm,
                destination,
                first,
                second,
                mask,
            },
        )
    }
    pub(crate) fn operation(self) -> RearrangeOperation {
        self.operation
    }
    pub(crate) fn with_width(mut self, width: VectorWidth) -> Option<Self> {
        if width == VectorWidth::Ymm && self.operation.kind().widening().is_none() {
            return None;
        }
        self.width = width;
        Some(self)
    }
    pub(crate) fn width(self) -> VectorWidth {
        self.width
    }
    pub(crate) fn source_bytes(self) -> u8 {
        self.operation.memory_bytes() * (self.width.bytes() / 16) as u8
    }
    pub(crate) fn destination(self) -> VectorRegister {
        self.destination
    }
    pub(crate) fn first(self) -> Option<VectorRegister> {
        self.first
    }
    pub(crate) fn second(self) -> MoveSource {
        self.second
    }
    pub(crate) fn mask(self) -> Option<VectorRegister> {
        self.mask
    }
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        (self.second == MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: self.source_bytes(),
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_rearrange(
        &mut self,
        instruction: RearrangeInstruction,
        operand: &[u8],
    ) -> Result<(), RearrangeError> {
        let bytes = instruction.memory().map_or(0, |r| usize::from(r.bytes));
        if operand.len() != bytes {
            return Err(RearrangeError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(RearrangeError::InvalidState);
        }
        let first = instruction
            .first
            .map_or([0; 16], |r| self.simd.xmm[r.index()]);
        let second = match instruction.second {
            MoveSource::Register(r) => self.simd.xmm[r.index()],
            MoveSource::Memory => {
                let mut value = [0; 16];
                value[..bytes].copy_from_slice(operand);
                value
            }
        };
        let mask = instruction
            .mask
            .map_or([0; 16], |r| self.simd.xmm[r.index()]);
        let mut value = [0; 32];
        value[..16].copy_from_slice(&instruction.operation.evaluate(first, second, mask));
        if instruction.width == VectorWidth::Ymm {
            let offset = usize::from(instruction.source_bytes() / 2);
            let mut next = [0; 16];
            next[..16 - offset].copy_from_slice(&second[offset..]);
            value[16..].copy_from_slice(&instruction.operation.evaluate(first, next, mask));
        }
        self.publish(instruction.destination, value);
        Ok(())
    }
}

#[cfg(test)]
#[path = "rearrange/native.rs"]
mod tests;

#[cfg(test)]
#[path = "rearrange/widen/native.rs"]
mod widen_tests;
