use super::descriptors::SegmentLoad;
use super::{SegmentContext, SegmentDescriptorTable, SegmentRegister};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarWidth {
    Word,
    Dword,
}

impl FarWidth {
    pub const fn offset(self, offset: u32) -> u32 {
        match self {
            Self::Word => offset & 0xffff,
            Self::Dword => offset,
        }
    }

    pub const fn pointer_bytes(self) -> u32 {
        match self {
            Self::Word => 4,
            Self::Dword => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeTransferError {
    GeneralProtection(u32),
    NotPresent(u32),
    UnsupportedProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeTransfer {
    pub offset: u32,
    load: SegmentLoad,
}

impl CodeTransfer {
    pub fn selector(self) -> u16 {
        self.load.selector
    }

    pub fn apply(self, context: &mut SegmentContext) {
        self.load.apply_context(context, SegmentRegister::Cs);
    }
}

impl SegmentDescriptorTable {
    /// resolve direct far JMP and the outer system-descriptor checks before a profile refusal.
    /// resolution never sets accessed or publishes the loaded code cache.
    pub fn resolve_far_jump(
        &self,
        selector: u16,
        offset: u32,
        width: FarWidth,
        cpl: u8,
    ) -> Result<CodeTransfer, CodeTransferError> {
        let error = u32::from(selector & !3);
        if error == 0 {
            return Err(CodeTransferError::GeneralProtection(0));
        }
        let record = self
            .raw_descriptor(selector)
            .copied()
            .ok_or(CodeTransferError::GeneralProtection(error))?;
        if !record.code_data() {
            // busy tss, reserved types, and a direct tss selector in the ldt are
            // architectural faults even though task/gate execution is outside this profile.
            match record.descriptor_type() {
                1 | 9 if selector & 4 == 0 => {}
                4 | 5 | 12 => {}
                _ => return Err(CodeTransferError::GeneralProtection(error)),
            }
            if cpl > 3 || record.dpl() < cpl || record.dpl() < (selector & 3) as u8 {
                return Err(CodeTransferError::GeneralProtection(error));
            }
            if !record.present() {
                return Err(CodeTransferError::NotPresent(error));
            }
            // outer-admissible does not establish a valid gate payload or task.
            // referenced selectors/state remain unresolved behind a neutral host refusal.
            return Err(CodeTransferError::UnsupportedProfile);
        }
        let kind = record.descriptor_type();
        if kind & 8 == 0 {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        let conforming = kind & 4 != 0;
        if cpl > 3
            || if conforming {
                record.dpl() > cpl
            } else {
                record.dpl() != cpl || (selector & 3) as u8 > cpl
            }
        {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        if !record.present() {
            return Err(CodeTransferError::NotPresent(error));
        }
        let offset = width.offset(offset);
        let limit = if record.granularity() {
            (record.raw_limit() << 12) | 0xfff
        } else {
            record.raw_limit()
        };
        if offset > limit {
            return Err(CodeTransferError::GeneralProtection(0));
        }
        // captured blocks use linear EIP and a fixed 32-bit decoder. A different base, mode,
        // or fetch bound must remain a host profile refusal until that execution model exists.
        if record.code_data_base() != Some(0)
            || !record.default32()
            || record.long_mode()
            || limit != u32::MAX
        {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        Ok(CodeTransfer {
            offset,
            load: SegmentLoad {
                selector: (selector & !3) | u16::from(cpl),
                base: 0,
                cache: record
                    .loaded_cache()
                    .expect("present non-long code is cacheable"),
            },
        })
    }
}

#[cfg(test)]
mod tests;
