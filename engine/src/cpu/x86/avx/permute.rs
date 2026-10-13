use super::{
    Element, MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth, WordTarget,
};
use crate::{cpu::x86::State32, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermuteInstruction {
    Interleave {
        high: bool,
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
    },
    Shuffle {
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    Immediate {
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        source: MoveSource,
        immediate: u8,
    },
    Variable {
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        source: VectorRegister,
        control: MoveSource,
    },
    Blend {
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    BlendVariable {
        element: Element,
        width: VectorWidth,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        mask: VectorRegister,
    },
    InsertSingle {
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    ExtractSingle {
        source: VectorRegister,
        destination: WordTarget,
        immediate: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermuteError {
    OperandLength,
    InvalidState,
}

impl PermuteInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        use PermuteInstruction::*;
        let (access, bytes) = match self {
            Interleave {
                width,
                second: MoveSource::Memory,
                ..
            }
            | Shuffle {
                width,
                second: MoveSource::Memory,
                ..
            }
            | Immediate {
                width,
                source: MoveSource::Memory,
                ..
            }
            | Variable {
                width,
                control: MoveSource::Memory,
                ..
            }
            | Blend {
                width,
                second: MoveSource::Memory,
                ..
            }
            | BlendVariable {
                width,
                second: MoveSource::Memory,
                ..
            } => (Access::Read, width.bytes() as u8),
            InsertSingle {
                second: MoveSource::Memory,
                ..
            } => (Access::Read, 4),
            ExtractSingle {
                destination: WordTarget::Memory,
                ..
            } => (Access::Write, 4),
            _ => return None,
        };
        Some(MemoryRequirement {
            access,
            bytes,
            alignment: 1,
        })
    }
}

fn input(state: &VectorState, source: MoveSource, operand: &[u8]) -> [u8; 32] {
    match source {
        MoveSource::Register(register) => state.read(register),
        MoveSource::Memory => {
            let mut value = [0; 32];
            value[..operand.len()].copy_from_slice(operand);
            value
        }
    }
}
fn lane(
    result: &mut [u8; 32],
    destination: usize,
    source: &[u8; 32],
    source_offset: usize,
    bytes: usize,
) {
    result[destination..destination + bytes]
        .copy_from_slice(&source[source_offset..source_offset + bytes]);
}

impl VectorState {
    // operand authority/currentness is checked by the shared caller before entry.
    // raw bit permutation never evaluates signaling NaNs or modifies MXCSR/FLAGS.
    pub(crate) fn execute_permute(
        &mut self,
        operation: PermuteInstruction,
        general: &mut State32,
        operand: &mut [u8],
    ) -> Result<(), PermuteError> {
        if operand.len() != operation.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(PermuteError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(PermuteError::InvalidState);
        }
        let mut value = [0; 32];
        use PermuteInstruction::*;
        let destination = match operation {
            Interleave {
                high,
                element,
                width,
                destination,
                first,
                second,
            } => {
                let a = self.read(first);
                let b = input(self, second, operand);
                let bytes = element.bytes();
                for half in (0..width.bytes()).step_by(16) {
                    for index in 0..8 / bytes {
                        let source = half + (index + if high { 8 / bytes } else { 0 }) * bytes;
                        lane(&mut value, half + index * 2 * bytes, &a, source, bytes);
                        lane(
                            &mut value,
                            half + (index * 2 + 1) * bytes,
                            &b,
                            source,
                            bytes,
                        );
                    }
                }
                Some(destination)
            }
            Shuffle {
                element,
                width,
                destination,
                first,
                second,
                immediate,
            } => {
                let a = self.read(first);
                let b = input(self, second, operand);
                let bytes = element.bytes();
                for half in 0..width.bytes() / 16 {
                    for index in 0..16 / bytes {
                        let selected = if element == Element::Single {
                            (immediate >> (index * 2)) & 3
                        } else {
                            (immediate >> (half * 2 + index)) & 1
                        };
                        let source = if index < 8 / bytes { &a } else { &b };
                        lane(
                            &mut value,
                            half * 16 + index * bytes,
                            source,
                            half * 16 + usize::from(selected) * bytes,
                            bytes,
                        );
                    }
                }
                Some(destination)
            }
            Immediate {
                element,
                width,
                destination,
                source,
                immediate,
            } => {
                let source = input(self, source, operand);
                let bytes = element.bytes();
                for index in 0..width.bytes() / bytes {
                    let selected = if element == Element::Single {
                        (immediate >> ((index % 4) * 2)) & 3
                    } else {
                        (immediate >> index) & 1
                    };
                    lane(
                        &mut value,
                        index * bytes,
                        &source,
                        index * bytes / 16 * 16 + usize::from(selected) * bytes,
                        bytes,
                    );
                }
                Some(destination)
            }
            Variable {
                element,
                width,
                destination,
                source,
                control,
            } => {
                let source = self.read(source);
                let control = input(self, control, operand);
                let bytes = element.bytes();
                for index in 0..width.bytes() / bytes {
                    // vex vpermilpd uses bit1 of each qword, per SDM operation.
                    let selected = if element == Element::Single {
                        control[index * bytes] & 3
                    } else {
                        (control[index * bytes] >> 1) & 1
                    };
                    lane(
                        &mut value,
                        index * bytes,
                        &source,
                        index * bytes / 16 * 16 + usize::from(selected) * bytes,
                        bytes,
                    );
                }
                Some(destination)
            }
            Blend {
                element,
                width,
                destination,
                first,
                second,
                immediate,
            } => {
                let a = self.read(first);
                let b = input(self, second, operand);
                let bytes = element.bytes();
                for index in 0..width.bytes() / bytes {
                    lane(
                        &mut value,
                        index * bytes,
                        if immediate & (1 << index) == 0 {
                            &a
                        } else {
                            &b
                        },
                        index * bytes,
                        bytes,
                    );
                }
                Some(destination)
            }
            BlendVariable {
                element,
                width,
                destination,
                first,
                second,
                mask,
            } => {
                let a = self.read(first);
                let b = input(self, second, operand);
                let mask = self.read(mask);
                let bytes = element.bytes();
                for index in 0..width.bytes() / bytes {
                    lane(
                        &mut value,
                        index * bytes,
                        if mask[(index + 1) * bytes - 1] & 0x80 == 0 {
                            &a
                        } else {
                            &b
                        },
                        index * bytes,
                        bytes,
                    );
                }
                Some(destination)
            }
            InsertSingle {
                destination,
                first,
                second,
                immediate,
            } => {
                value[..16].copy_from_slice(&self.read(first)[..16]);
                let source = input(self, second, operand);
                let source_index = if second == MoveSource::Memory {
                    0
                } else {
                    usize::from(immediate >> 6) * 4
                };
                lane(
                    &mut value,
                    usize::from(immediate >> 4 & 3) * 4,
                    &source,
                    source_index,
                    4,
                );
                for index in 0..4 {
                    if immediate & (1 << index) != 0 {
                        value[index * 4..index * 4 + 4].fill(0);
                    }
                }
                Some(destination)
            }
            ExtractSingle {
                source,
                destination,
                immediate,
            } => {
                let source = self.read(source);
                let offset = usize::from(immediate & 3) * 4;
                match destination {
                    WordTarget::Register(register) => {
                        general.registers[register.index()] =
                            u32::from_le_bytes(source[offset..offset + 4].try_into().unwrap())
                    }
                    WordTarget::Memory => operand.copy_from_slice(&source[offset..offset + 4]),
                }
                None
            }
        };
        if let Some(destination) = destination {
            self.publish(destination, value);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "permute/native.rs"]
mod tests;
