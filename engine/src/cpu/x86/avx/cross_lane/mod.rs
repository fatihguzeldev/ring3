use super::{
    ArithmeticError, MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth,
};
use crate::memory::Access;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockTarget {
    Register(VectorRegister),
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BroadcastElement {
    Byte,
    Word,
    Dword,
    Qword,
}

impl BroadcastElement {
    pub(crate) fn bytes(self) -> u8 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Dword => 4,
            Self::Qword => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrossLaneInstruction {
    Broadcast {
        element: BroadcastElement,
        width: VectorWidth,
        destination: VectorRegister,
        source: MoveSource,
    },
    BroadcastBlock {
        destination: VectorRegister,
    },
    Permute {
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    Insert {
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    Extract {
        source: VectorRegister,
        destination: BlockTarget,
        immediate: u8,
    },
}

impl CrossLaneInstruction {
    pub(crate) fn requires_avx2(self) -> bool {
        matches!(
            self,
            Self::Broadcast {
                source: MoveSource::Register(_),
                ..
            } | Self::Broadcast {
                element: BroadcastElement::Byte | BroadcastElement::Word,
                ..
            } | Self::Broadcast {
                element: BroadcastElement::Qword,
                width: VectorWidth::Xmm,
                ..
            }
        )
    }

    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let (access, bytes) = match self {
            Self::Broadcast {
                element,
                source: MoveSource::Memory,
                ..
            } => (Access::Read, element.bytes()),
            Self::BroadcastBlock { .. }
            | Self::Insert {
                second: MoveSource::Memory,
                ..
            } => (Access::Read, 16),
            Self::Permute {
                second: MoveSource::Memory,
                ..
            } => (Access::Read, 32),
            Self::Extract {
                destination: BlockTarget::Memory,
                ..
            } => (Access::Write, 16),
            _ => return None,
        };
        Some(MemoryRequirement {
            access,
            bytes,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_cross_lane(
        &mut self,
        operation: CrossLaneInstruction,
        operand: &mut [u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != operation.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let mut result = [0; 32];
        let (destination, first, second, immediate) = match operation {
            CrossLaneInstruction::Broadcast {
                element,
                width,
                destination,
                source,
            } => {
                let source = self.capture_source(source, operand);
                let bytes = usize::from(element.bytes());
                for lane in result[..width.bytes()].chunks_exact_mut(bytes) {
                    lane.copy_from_slice(&source[..bytes]);
                }
                self.publish(destination, result);
                return Ok(());
            }
            CrossLaneInstruction::BroadcastBlock { destination } => {
                result[..16].copy_from_slice(operand);
                result[16..].copy_from_slice(operand);
                self.publish(destination, result);
                return Ok(());
            }
            CrossLaneInstruction::Extract {
                source,
                destination,
                immediate,
            } => {
                let source = self.read(source);
                let offset = usize::from(immediate & 1) * 16;
                match destination {
                    BlockTarget::Register(destination) => {
                        result[..16].copy_from_slice(&source[offset..offset + 16]);
                        self.publish(destination, result);
                    }
                    BlockTarget::Memory => operand.copy_from_slice(&source[offset..offset + 16]),
                }
                return Ok(());
            }
            CrossLaneInstruction::Insert {
                destination,
                first,
                second,
                immediate,
            }
            | CrossLaneInstruction::Permute {
                destination,
                first,
                second,
                immediate,
            } => (destination, first, second, immediate),
        };
        let first = self.read(first);
        let second = match second {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..operand.len()].copy_from_slice(operand);
                value
            }
        };
        if matches!(operation, CrossLaneInstruction::Insert { .. }) {
            result = first;
            let offset = usize::from(immediate & 1) * 16;
            result[offset..offset + 16].copy_from_slice(&second[..16]);
        } else {
            for half in 0..2 {
                let control = immediate >> (half * 4);
                if control & 8 == 0 {
                    let source = if control & 2 == 0 { &first } else { &second };
                    let offset = usize::from(control & 1) * 16;
                    result[half * 16..half * 16 + 16].copy_from_slice(&source[offset..offset + 16]);
                }
            }
        }
        self.publish(destination, result);
        Ok(())
    }
}

#[cfg(test)]
mod native;
