use crate::cpu::x86::{Register32, SegmentRegister};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherAddress {
    base: Option<Register32>,
    displacement: u32,
    segment: Option<SegmentRegister>,
}

impl GatherAddress {
    pub(crate) const fn new(
        base: Option<Register32>,
        displacement: u32,
        segment: Option<SegmentRegister>,
    ) -> Self {
        Self {
            base,
            displacement,
            segment,
        }
    }

    pub(crate) const fn base(self) -> Option<Register32> {
        self.base
    }

    pub(crate) const fn displacement(self) -> u32 {
        self.displacement
    }

    pub(crate) const fn resolved_segment(self) -> SegmentRegister {
        match self.segment {
            Some(segment) => segment,
            None if matches!(self.base, Some(Register32::Esp | Register32::Ebp)) => {
                SegmentRegister::Ss
            }
            None => SegmentRegister::Ds,
        }
    }
}

#[cfg(test)]
#[path = "address/native.rs"]
mod tests;
