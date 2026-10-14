use super::InterruptDescriptorTable32;
use crate::cpu::x86::{CodeDestination, CodeTransferError, FarWidth, SegmentDescriptorTable};

/// an admitted configured IDT payload; no target cache or accessed bit is published yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InterruptGate {
    selector: u16,
    offset: u32,
    width: FarWidth,
    interrupt: bool,
}

impl InterruptDescriptorTable32 {
    pub(crate) fn resolve_gate(
        &self,
        vector: u8,
        cpl: u8,
    ) -> Result<InterruptGate, CodeTransferError> {
        let error = (u32::from(vector) << 3) | 2;
        if u32::from(vector) * 8 + 7 > u32::from(self.limit()) {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        let record = *self.record(vector);
        let kind = record.descriptor_type();
        if record.code_data() || !matches!(kind, 5 | 6 | 7 | 14 | 15) {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        if cpl > 3 || record.dpl() < cpl {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        if !record.present() {
            return Err(CodeTransferError::NotPresent(error));
        }
        // outer architectural checks are meaningful even for unimplemented task gates.
        if kind == 5 {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        let (low, high) = record.to_words();
        let width = if matches!(kind, 6 | 7) {
            FarWidth::Word
        } else {
            FarWidth::Dword
        };
        // configured reserved payload bits have no invented architectural fault policy.
        if high & 0xff != 0 || (width == FarWidth::Word && high & 0xffff0000 != 0) {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        Ok(InterruptGate {
            selector: (low >> 16) as u16,
            offset: width.offset((low & 0xffff) | (high & 0xffff0000)),
            width,
            interrupt: matches!(kind, 6 | 14),
        })
    }
}

impl InterruptGate {
    pub(crate) const fn width(self) -> FarWidth {
        self.width
    }
    pub(crate) const fn offset(self) -> u32 {
        self.offset
    }
    pub(crate) const fn frame_info(self) -> u32 {
        self.width.slot_bytes() | if self.interrupt { 0x100 } else { 0 }
    }

    /// embedded RPL is ignored. Descriptor admission ends before SS capacity and target limit.
    pub(crate) fn destination(
        self,
        table: &SegmentDescriptorTable,
        cpl: u8,
    ) -> Result<CodeDestination, CodeTransferError> {
        let error = u32::from(self.selector & !3);
        if error == 0 {
            return Err(CodeTransferError::GeneralProtection(0));
        }
        let record = table
            .raw_descriptor(self.selector)
            .copied()
            .ok_or(CodeTransferError::GeneralProtection(error))?;
        let kind = record.descriptor_type();
        if cpl > 3 || !record.code_data() || kind & 8 == 0 || record.dpl() > cpl {
            return Err(CodeTransferError::GeneralProtection(error));
        }
        if !record.present() {
            return Err(CodeTransferError::NotPresent(error));
        }
        if kind & 4 == 0 && record.dpl() < cpl {
            return Err(CodeTransferError::UnsupportedProfile);
        }
        // the existing direct-code resolver owns cache construction. Normalizing RPL only
        // after the interrupt-specific checks preserves embedded-selector semantics.
        table.resolve_far_call((self.selector & !3) | u16::from(cpl), cpl)
    }
}

#[cfg(test)]
mod tests;
