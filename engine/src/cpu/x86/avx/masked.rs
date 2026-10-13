use super::{Element, VectorRegister, VectorState, VectorWidth};
use crate::{
    cpu::x86::{SegmentAccess, SegmentAccessError, SegmentContext, SegmentRegister},
    memory::{Access, AddressSpace, GuestAddress, MemoryError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaskedInstruction {
    Load {
        width: VectorWidth,
        element: Element,
        destination: VectorRegister,
        mask: VectorRegister,
    },
    Store {
        width: VectorWidth,
        element: Element,
        source: VectorRegister,
        mask: VectorRegister,
    },
    StoreBytes {
        source: VectorRegister,
        mask: VectorRegister,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MaskedError {
    InvalidState,
    Segment {
        segment: SegmentRegister,
        cause: SegmentAccessError,
    },
    Memory(MemoryError),
}

impl MaskedInstruction {
    pub(crate) fn width(self) -> VectorWidth {
        match self {
            Self::Load { width, .. } | Self::Store { width, .. } => width,
            Self::StoreBytes { .. } => VectorWidth::Xmm,
        }
    }
    pub(crate) fn lane_bytes(self) -> usize {
        match self {
            Self::Load { element, .. } | Self::Store { element, .. } => element.bytes(),
            Self::StoreBytes { .. } => 1,
        }
    }
    pub(crate) fn mask(self) -> VectorRegister {
        match self {
            Self::Load { mask, .. } | Self::Store { mask, .. } | Self::StoreBytes { mask, .. } => {
                mask
            }
        }
    }
    pub(crate) fn access(self) -> Access {
        if matches!(self, Self::Load { .. }) {
            Access::Read
        } else {
            Access::Write
        }
    }
    pub(crate) fn selected_bytes(self, mask: [u8; 32]) -> u32 {
        let mut selected = 0;
        for start in (0..self.width().bytes()).step_by(self.lane_bytes()) {
            if mask[start + self.lane_bytes() - 1] & 0x80 != 0 {
                for byte in start..start + self.lane_bytes() {
                    selected |= 1u32 << byte;
                }
            }
        }
        selected
    }
}

impl VectorState {
    pub(crate) fn execute_masked(
        &mut self,
        instruction: MaskedInstruction,
        segments: &SegmentContext,
        segment: SegmentRegister,
        offset: u32,
        memory: &mut AddressSpace,
    ) -> Result<(), MaskedError> {
        if self.simd.mxcsr & !0xffff != 0 || !segments.is_valid() {
            return Err(MaskedError::InvalidState);
        }
        let mask = self.read(instruction.mask());
        let selected = instruction.selected_bytes(mask);
        let data = match instruction {
            MaskedInstruction::Store { source, .. }
            | MaskedInstruction::StoreBytes { source, .. } => self.read(source),
            _ => [0; 32],
        };
        let access = if instruction.access() == Access::Read {
            SegmentAccess::Read
        } else {
            SegmentAccess::Write
        };
        for start in (0..instruction.width().bytes()).step_by(instruction.lane_bytes()) {
            if selected & (1u32 << start) != 0 {
                let logical = offset
                    .checked_add(start as u32)
                    .ok_or(MaskedError::Segment {
                        segment,
                        cause: SegmentAccessError::Limit,
                    })?;
                segments.caches[segment.index()]
                    .check_span(logical, instruction.lane_bytes() as u32, access)
                    .map_err(|cause| MaskedError::Segment { segment, cause })?;
            }
        }
        let address = GuestAddress(segments.state.bases[segment.index()].wrapping_add(offset));
        match instruction {
            MaskedInstruction::Load { destination, .. } => {
                let mut value = [0; 32];
                memory
                    .read_masked(address, &mut value[..instruction.width().bytes()], selected)
                    .map_err(MaskedError::Memory)?;
                self.publish(destination, value);
            }
            _ => memory
                .write_masked(address, &data[..instruction.width().bytes()], selected)
                .map_err(MaskedError::Memory)?,
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "masked/native.rs"]
mod tests;
