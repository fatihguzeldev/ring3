#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentRegister {
    Es,
    Cs,
    Ss,
    Ds,
    Fs,
    Gs,
}

impl SegmentRegister {
    pub const fn index(self) -> usize {
        match self {
            Self::Es => 0,
            Self::Cs => 1,
            Self::Ss => 2,
            Self::Ds => 3,
            Self::Fs => 4,
            Self::Gs => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentState {
    pub selectors: [u16; 6],
    pub bases: [u32; 6],
}

impl Default for SegmentState {
    fn default() -> Self {
        Self {
            selectors: [0x23, 0x1b, 0x23, 0x23, 0x3b, 0],
            bases: [0; 6],
        }
    }
}

impl SegmentState {
    pub fn set_base(&mut self, register: SegmentRegister, base: u32) -> bool {
        if !matches!(register, SegmentRegister::Fs | SegmentRegister::Gs) && base != 0 {
            return false;
        }
        self.bases[register.index()] = base;
        true
    }

    pub fn is_valid(&self) -> bool {
        self.bases[SegmentRegister::Cs.index()] == 0
            && self.bases[SegmentRegister::Ss.index()] == 0
            && [SegmentRegister::Cs, SegmentRegister::Ss]
                .into_iter()
                .all(|segment| {
                    let selector = self.selectors[segment.index()];
                    selector & !3 != 0 && selector & 3 == 3
                })
    }
}
mod cache;
mod descriptor_record;
mod descriptors;
mod transfers;
pub use descriptor_record::SegmentDescriptorRecord;
pub use transfers::{CodeDestination, CodeTransfer, CodeTransferError, FarWidth};

pub use cache::{SegmentAccess, SegmentAccessError, SegmentCache, SegmentContext};

pub use descriptors::{
    SegmentDescriptor, SegmentDescriptorKind, SegmentDescriptorTable, SegmentLoadError,
    SegmentLoadFailure,
};

mod queries;
pub use queries::SegmentQuery;
