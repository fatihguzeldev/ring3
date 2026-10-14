use super::{VectorRegister, VectorState};
use crate::{
    cpu::x86::{SegmentAccess, SegmentAccessError, SegmentContext, SegmentRegister},
    memory::{AddressSpace, GuestAddress, MemoryError, MemoryFault},
};

mod address;
mod geometry;
pub(crate) use address::GatherAddress;
pub(crate) use geometry::{GatherGeometry, GatherScale};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherInstruction {
    geometry: GatherGeometry,
    destination: VectorRegister,
    index: VectorRegister,
    mask: VectorRegister,
    scale: GatherScale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherInputs {
    pub(crate) destination: [u8; 32],
    pub(crate) mask: [u8; 32],
    pub(crate) index: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherOutcome {
    pub(crate) destination: [u8; 32],
    pub(crate) mask: [u8; 32],
    pub(crate) completed_lanes: u8,
    pub(crate) fault: Option<GatherFault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherFault {
    pub(crate) lane: u8,
    pub(crate) cause: GatherFaultCause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherFaultCause {
    Segment {
        segment: SegmentRegister,
        cause: SegmentAccessError,
    },
    Memory(MemoryFault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherError {
    InvalidContext,
    Infrastructure(MemoryError),
}

impl GatherInstruction {
    pub(crate) fn new(
        geometry: GatherGeometry,
        destination: VectorRegister,
        index: VectorRegister,
        mask: VectorRegister,
        scale: GatherScale,
    ) -> Option<Self> {
        if destination == index || destination == mask || index == mask {
            return None;
        }
        Some(Self {
            geometry,
            destination,
            index,
            mask,
            scale,
        })
    }

    pub(crate) const fn geometry(self) -> GatherGeometry {
        self.geometry
    }
    pub(crate) const fn destination(self) -> VectorRegister {
        self.destination
    }
    pub(crate) const fn index(self) -> VectorRegister {
        self.index
    }
    pub(crate) const fn mask(self) -> VectorRegister {
        self.mask
    }
    pub(crate) const fn scale(self) -> GatherScale {
        self.scale
    }

    pub(crate) fn capture(self, vectors: &VectorState) -> GatherInputs {
        GatherInputs {
            destination: vectors.read(self.destination),
            mask: vectors.read(self.mask),
            index: vectors.read(self.index),
        }
    }

    pub(crate) fn selected(self, mask: &[u8; 32]) -> u8 {
        let bytes = self.geometry.data_bytes();
        (0..self.geometry.lanes()).fold(0, |selected, lane| {
            selected | (u8::from(mask[(lane + 1) * bytes - 1] & 0x80 != 0) << lane)
        })
    }

    /// produce a staged result even for an ordinary operand fault; never publish vector state.
    pub(crate) fn evaluate(
        self,
        inputs: GatherInputs,
        segment: SegmentRegister,
        base_offset: u32,
        segments: Option<&SegmentContext>,
        memory: &AddressSpace,
    ) -> Result<GatherOutcome, GatherError> {
        self.evaluate_context(
            inputs,
            segment,
            base_offset,
            segments,
            memory,
            segments.is_some_and(SegmentContext::is_valid),
        )
    }

    pub(crate) fn evaluate_task(
        self,
        inputs: GatherInputs,
        segment: SegmentRegister,
        base_offset: u32,
        context: &crate::cpu::x86::tasks::TaskIncomingContext32,
        memory: &AddressSpace,
    ) -> Result<GatherOutcome, GatherError> {
        self.evaluate_context(
            inputs,
            segment,
            base_offset,
            Some(&context.segment_image()),
            memory,
            crate::cpu::x86::tasks::TaskIncomingContext32::from_image(context.image()).is_some(),
        )
    }

    fn evaluate_context(
        self,
        inputs: GatherInputs,
        segment: SegmentRegister,
        base_offset: u32,
        segments: Option<&SegmentContext>,
        memory: &AddressSpace,
        context_valid: bool,
    ) -> Result<GatherOutcome, GatherError> {
        let geometry = self.geometry;
        let bytes = geometry.data_bytes();
        let selected = self.selected(&inputs.mask);
        let mut outcome = GatherOutcome {
            destination: inputs.destination,
            mask: [0; 32],
            completed_lanes: 0,
            fault: None,
        };
        if geometry.early_zero_unused_destination() {
            outcome.destination[geometry.active_bytes()..].fill(0);
        }
        for lane in 0..geometry.lanes() {
            if selected & (1 << lane) != 0 {
                outcome.mask[lane * bytes..(lane + 1) * bytes].fill(0xff);
            }
        }
        if selected == 0 {
            outcome.destination[geometry.active_bytes()..].fill(0);
            return Ok(outcome);
        }
        let segments = segments
            .filter(|_| context_valid)
            .ok_or(GatherError::InvalidContext)?;
        // ascending order is the software implementation's repeatable completion policy.
        // segment and page checks belong to each lane, never a whole-gather preflight.
        for lane in 0..geometry.lanes() {
            if selected & (1 << lane) == 0 {
                continue;
            }
            let offset = self.lane_offset(&inputs.index, lane, base_offset);
            if let Err(cause) = segments.caches[segment.index()].check_span(
                offset,
                bytes as u32,
                SegmentAccess::Read,
            ) {
                outcome.fault = Some(GatherFault {
                    lane: lane as u8,
                    cause: GatherFaultCause::Segment { segment, cause },
                });
                return Ok(outcome);
            }
            let address = GuestAddress(segments.state.bases[segment.index()].wrapping_add(offset));
            let mut value = [0; 8];
            match memory.cpu_read(address, &mut value[..bytes]) {
                Ok(()) => {}
                Err(MemoryError::Fault(fault)) => {
                    outcome.fault = Some(GatherFault {
                        lane: lane as u8,
                        cause: GatherFaultCause::Memory(fault),
                    });
                    return Ok(outcome);
                }
                Err(error) => return Err(GatherError::Infrastructure(error)),
            }
            outcome.destination[lane * bytes..(lane + 1) * bytes].copy_from_slice(&value[..bytes]);
            outcome.mask[lane * bytes..(lane + 1) * bytes].fill(0);
            outcome.completed_lanes |= 1 << lane;
        }
        outcome.destination[geometry.active_bytes()..].fill(0);
        Ok(outcome)
    }

    fn lane_offset(self, index: &[u8; 32], lane: usize, base: u32) -> u32 {
        let at = lane * self.geometry.index_bytes();
        let signed = if self.geometry.index_bytes() == 4 {
            i64::from(i32::from_le_bytes(index[at..at + 4].try_into().unwrap()))
        } else {
            i64::from_le_bytes(index[at..at + 8].try_into().unwrap())
        };
        // the start address wraps at 32 bits. the element span is checked separately above.
        base.wrapping_add((signed as u32).wrapping_mul(self.scale.multiplier()))
    }
}

#[cfg(test)]
#[path = "native.rs"]
mod tests;
