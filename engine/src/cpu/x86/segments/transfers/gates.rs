use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gate {
    pub(super) offset: u32,
    pub(super) width: FarWidth,
    selector: u16,
}

pub(super) fn outer(
    record: SegmentDescriptorRecord,
    selector: u16,
    cpl: u8,
) -> Result<Gate, CodeTransferError> {
    let error = u32::from(selector & !3);
    if error == 0 || cpl > 3 || record.dpl() < cpl || record.dpl() < (selector & 3) as u8 {
        return Err(CodeTransferError::GeneralProtection(error));
    }
    if !record.present() {
        return Err(CodeTransferError::NotPresent(error));
    }
    let (low, high) = record.to_words();
    let width = match record.descriptor_type() {
        4 => FarWidth::Word,
        12 => FarWidth::Dword,
        _ => unreachable!("call-gate type checked by caller"),
    };
    // only canonical configured legacy payloads are admitted. Reserved-bit treatment
    // stays a neutral profile boundary instead of an invented architectural fault.
    if high & 0xe0 != 0 || (width == FarWidth::Word && high & 0xffff0000 != 0) {
        return Err(CodeTransferError::UnsupportedProfile);
    }
    Ok(Gate {
        offset: width.offset((low & 0xffff) | (high & 0xffff0000)),
        width,
        selector: (low >> 16) as u16,
    })
}

pub(super) fn destination(
    table: &SegmentDescriptorTable,
    record: SegmentDescriptorRecord,
    selector: u16,
    cpl: u8,
    call: bool,
) -> Result<CodeDestination, CodeTransferError> {
    let gate = outer(record, selector, cpl)?;
    let error = u32::from(gate.selector & !3);
    if error == 0 {
        return Err(CodeTransferError::GeneralProtection(0));
    }
    let target = table
        .raw_descriptor(gate.selector)
        .copied()
        .ok_or(CodeTransferError::GeneralProtection(error))?;
    let conforming = target.descriptor_type() & 4 != 0;
    if !target.code_data()
        || target.descriptor_type() & 8 == 0
        || target.dpl() > cpl
        || (!call && !conforming && target.dpl() != cpl)
    {
        return Err(CodeTransferError::GeneralProtection(error));
    }
    if !target.present() {
        return Err(CodeTransferError::NotPresent(error));
    }
    if call && !conforming && target.dpl() < cpl {
        return Err(CodeTransferError::UnsupportedProfile);
    }
    Ok(CodeDestination {
        record: target,
        selector: (gate.selector & !3) | u16::from(cpl),
        gate: Some(gate),
    })
}

#[cfg(test)]
mod tests;
