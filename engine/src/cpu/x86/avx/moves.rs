use super::{Element, VectorRegister, VectorState, VectorWidth};
use crate::{cpu::x86::{Register32, State32}, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveSource {
    Register(VectorRegister),
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordSource {
    Register(Register32),
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordTarget {
    Register(Register32),
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Half { Low, High }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HalfMerge { HighToLow, LowToHigh }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DuplicateKind { SingleLow, SingleHigh, DoubleLow }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveInstruction {
    Copy { width: VectorWidth, destination: VectorRegister, source: VectorRegister },
    Load { width: VectorWidth, destination: VectorRegister, aligned: bool },
    Store { width: VectorWidth, source: VectorRegister, aligned: bool },
    Scalar { element: Element, destination: VectorRegister, merge: VectorRegister, source: VectorRegister },
    LoadScalar { element: Element, destination: VectorRegister },
    StoreScalar { element: Element, source: VectorRegister },
    LoadHalf { half: Half, destination: VectorRegister, merge: VectorRegister },
    StoreHalf { half: Half, source: VectorRegister },
    MergeHalf { kind: HalfMerge, destination: VectorRegister, merge: VectorRegister, source: VectorRegister },
    Duplicate { kind: DuplicateKind, width: VectorWidth, destination: VectorRegister, source: MoveSource },
    DwordLoad { destination: VectorRegister, source: WordSource },
    DwordStore { source: VectorRegister, destination: WordTarget },
    QwordLoad { destination: VectorRegister, source: MoveSource },
    QwordStore { source: VectorRegister },
    Mask { element: Element, width: VectorWidth, destination: Register32, source: VectorRegister },
    LoadControl,
    StoreControl,
    ZeroUpper,
    ZeroAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryRequirement {
    pub access: Access,
    pub bytes: u8,
    pub alignment: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveError {
    OperandLength,
    Alignment,
    InvalidState,
    InvalidControl,
}

impl MoveInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        use MoveInstruction::*;
        let (access, bytes, aligned) = match self {
            Load { width, aligned, .. } => (Access::Read, width.bytes(), aligned),
            Store { width, aligned, .. } => (Access::Write, width.bytes(), aligned),
            LoadScalar { element, .. } => (Access::Read, element.bytes(), false),
            StoreScalar { element, .. } => (Access::Write, element.bytes(), false),
            LoadHalf { .. } | QwordLoad { source: MoveSource::Memory, .. } => (Access::Read, 8, false),
            StoreHalf { .. } | QwordStore { .. } => (Access::Write, 8, false),
            Duplicate { kind, width, source: MoveSource::Memory, .. } => {
                let bytes = if kind == DuplicateKind::DoubleLow && width == VectorWidth::Xmm { 8 } else { width.bytes() };
                (Access::Read, bytes, false)
            }
            DwordLoad { source: WordSource::Memory, .. } | LoadControl => (Access::Read, 4, false),
            DwordStore { destination: WordTarget::Memory, .. } | StoreControl => (Access::Write, 4, false),
            _ => return None,
        };
        Some(MemoryRequirement { access, bytes: bytes as u8, alignment: if aligned { bytes as u8 } else { 1 } })
    }
}

impl VectorState {
    // the operand is a checked exact span. the caller owns segment/page authority,
    // writable-code currentness and guest fault delivery before this state kernel.
    pub(crate) fn execute_move(
        &mut self,
        operation: MoveInstruction,
        general: &mut State32,
        operand_address: u32,
        operand: &mut [u8],
    ) -> Result<(), MoveError> {
        let requirement = operation.memory();
        if operand.len() != requirement.map_or(0, |r| usize::from(r.bytes)) {
            return Err(MoveError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(MoveError::InvalidState);
        }
        if requirement.is_some_and(|r| operand_address & (u32::from(r.alignment) - 1) != 0) {
            return Err(MoveError::Alignment);
        }
        use MoveInstruction::*;
        let mut value = [0; 32];
        let destination = match operation {
            Copy { width, destination, source } => {
                value[..width.bytes()].copy_from_slice(&self.read(source)[..width.bytes()]);
                Some(destination)
            }
            Load { destination, .. } | LoadScalar { destination, .. } => {
                value[..operand.len()].copy_from_slice(operand);
                Some(destination)
            }
            Store { source, .. } | StoreScalar { source, .. } | QwordStore { source } => {
                operand.copy_from_slice(&self.read(source)[..operand.len()]);
                None
            }
            Scalar { element, destination, merge, source } => {
                value[..16].copy_from_slice(&self.read(merge)[..16]);
                value[..element.bytes()].copy_from_slice(&self.read(source)[..element.bytes()]);
                Some(destination)
            }
            LoadHalf { half, destination, merge } => {
                value[..16].copy_from_slice(&self.read(merge)[..16]);
                let offset = if half == Half::Low { 0 } else { 8 };
                value[offset..offset + 8].copy_from_slice(operand);
                Some(destination)
            }
            StoreHalf { half, source } => {
                let offset = if half == Half::Low { 0 } else { 8 };
                operand.copy_from_slice(&self.read(source)[offset..offset + 8]);
                None
            }
            MergeHalf { kind, destination, merge, source } => {
                let first = self.read(merge);
                let second = self.read(source);
                match kind {
                    HalfMerge::HighToLow => {
                        value[..8].copy_from_slice(&second[8..16]);
                        value[8..16].copy_from_slice(&first[8..16]);
                    }
                    HalfMerge::LowToHigh => {
                        value[..8].copy_from_slice(&first[..8]);
                        value[8..16].copy_from_slice(&second[..8]);
                    }
                }
                Some(destination)
            }
            Duplicate { kind, width, destination, source } => {
                let input = match source {
                    MoveSource::Register(register) => self.read(register),
                    MoveSource::Memory => {
                        let mut input = [0; 32];
                        input[..operand.len()].copy_from_slice(operand);
                        input
                    }
                };
                for half in (0..width.bytes()).step_by(16) {
                    let (element, lanes): (usize, &[usize]) = match kind {
                        DuplicateKind::SingleLow => (4, &[0, 0, 2, 2]),
                        DuplicateKind::SingleHigh => (4, &[1, 1, 3, 3]),
                        DuplicateKind::DoubleLow => (8, &[0, 0]),
                    };
                    for (lane, &source_lane) in lanes.iter().enumerate() {
                        let destination = half + lane * element;
                        let source = half + source_lane * element;
                        value[destination..destination + element].copy_from_slice(&input[source..source + element]);
                    }
                }
                Some(destination)
            }
            DwordLoad { destination, source } => {
                value[..4].copy_from_slice(&match source {
                    WordSource::Register(register) => general.registers[register.index()].to_le_bytes(),
                    WordSource::Memory => operand[..4].try_into().unwrap(),
                });
                Some(destination)
            }
            DwordStore { source, destination } => {
                let input = self.read(source);
                match destination {
                    WordTarget::Register(register) => general.registers[register.index()] = u32::from_le_bytes(input[..4].try_into().unwrap()),
                    WordTarget::Memory => operand.copy_from_slice(&input[..4]),
                }
                None
            }
            QwordLoad { destination, source } => {
                let input = match source {
                    MoveSource::Register(register) => self.read(register),
                    MoveSource::Memory => {
                        let mut input = [0; 32];
                        input[..8].copy_from_slice(operand);
                        input
                    }
                };
                value[..8].copy_from_slice(&input[..8]);
                Some(destination)
            }
            Mask { element, width, destination, source } => {
                let input = self.read(source);
                let mut mask = 0;
                for lane in 0..width.bytes() / element.bytes() {
                    mask |= u32::from(input[(lane + 1) * element.bytes() - 1] >> 7) << lane;
                }
                general.registers[destination.index()] = mask;
                None
            }
            LoadControl => {
                let control = u32::from_le_bytes(operand[..4].try_into().unwrap());
                if control & !0xffff != 0 { return Err(MoveError::InvalidControl); }
                self.simd.mxcsr = control;
                None
            }
            StoreControl => { operand.copy_from_slice(&self.simd.mxcsr.to_le_bytes()); None }
            ZeroUpper => { self.upper = [[0; 16]; 8]; None }
            ZeroAll => { self.upper = [[0; 16]; 8]; self.simd.xmm = [[0; 16]; 8]; None }
        };
        if let Some(destination) = destination { self.publish(destination, value); }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
