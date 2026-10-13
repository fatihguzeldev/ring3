use super::descriptors::SegmentLoad;
use super::{SegmentContext, SegmentDescriptorRecord, SegmentDescriptorTable, SegmentRegister};

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

    pub const fn slot_bytes(self) -> u32 {
        match self {
            Self::Word => 2,
            Self::Dword => 4,
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

/// descriptor checks are complete, but target-limit checking is deliberately deferred.
/// far CALL must check return-stack capacity between these two phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeDestination {
    record: SegmentDescriptorRecord,
    selector: u16,
}

impl CodeDestination {
    pub fn target(self, offset: u32, width: FarWidth) -> Result<CodeTransfer, CodeTransferError> {
        let record = self.record;
        let offset = width.offset(offset);
        let limit = if record.granularity() {
            (record.raw_limit() << 12) | 0xfff
        } else {
            record.raw_limit()
        };
        if offset > limit {
            return Err(CodeTransferError::GeneralProtection(0));
        }
        // decoded code retains logical offsets under its loaded CS base and D key.
        if !record.default32() || record.long_mode() {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        Ok(CodeTransfer {
            offset,
            load: SegmentLoad {
                selector: self.selector,
                base: record.code_data_base().expect("code descriptor has a base"),
                cache: record
                    .loaded_cache()
                    .expect("present non-long code is cacheable"),
            },
        })
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
        self.direct_destination(selector, cpl)?
            .target(offset, width)
    }

    /// CALL's descriptor phase ends before the return-stack and target-limit checks.
    pub fn resolve_far_call(
        &self,
        selector: u16,
        cpl: u8,
    ) -> Result<CodeDestination, CodeTransferError> {
        self.direct_destination(selector, cpl)
    }

    /// RETF requires RPL >= CPL even for conforming code; it cannot select a gate or task.
    pub fn resolve_far_return(
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
        let rpl = (selector & 3) as u8;
        let kind = record.descriptor_type();
        if !record.code_data()
            || kind & 8 == 0
            || cpl > 3
            || rpl < cpl
            || if kind & 4 != 0 {
                record.dpl() > rpl
            } else {
                record.dpl() != rpl
            }
        {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        if !record.present() {
            return Err(CodeTransferError::NotPresent(error));
        }
        if rpl != cpl {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        CodeDestination { record, selector }.target(offset, width)
    }

    fn direct_destination(
        &self,
        selector: u16,
        cpl: u8,
    ) -> Result<CodeDestination, CodeTransferError> {
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
        Ok(CodeDestination {
            record,
            selector: (selector & !3) | u16::from(cpl),
        })
    }
}

#[cfg(test)]
mod tests;
