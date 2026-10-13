use super::{ADDRESS_LIMIT, Access, AddressSpace, CpuSpan, GuestAddress, MemoryError};

impl AddressSpace {
    pub(crate) fn cpu_write(
        &mut self,
        address: GuestAddress,
        input: &[u8],
    ) -> Result<(), MemoryError> {
        if input.len() as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.write(address, input);
        }
        let span = CpuSpan::new(address, input.len())?;
        self.cpu_check_span(address, input.len(), Access::Write)?;
        let version = self.advance_version()?;
        self.cpu_copy_in_validated(&span, input, version);
        Ok(())
    }

    pub(crate) fn cpu_write_spans(
        &mut self,
        spans: &[(GuestAddress, &[u8])],
    ) -> Result<(), MemoryError> {
        if spans.len() > 5 {
            return Err(MemoryError::InvalidRange);
        }
        if spans
            .iter()
            .all(|(address, bytes)| bytes.len() as u64 <= ADDRESS_LIMIT - u64::from(address.0))
        {
            return self.write_spans(spans);
        }
        let mut prepared: [Option<CpuSpan>; 5] = std::array::from_fn(|_| None);
        for (index, &(address, bytes)) in spans.iter().enumerate() {
            let span = CpuSpan::new(address, bytes.len())?;
            self.cpu_check_span(address, bytes.len(), Access::Write)?;
            prepared[index] = Some(span);
        }
        let version = self.advance_version()?;
        for ((_, bytes), span) in spans.iter().zip(prepared.iter().flatten()) {
            self.cpu_copy_in_validated(span, bytes, version);
        }
        Ok(())
    }

    pub(crate) fn cpu_write_stack(
        &mut self,
        addresses: &[u32],
        values: &[u32],
        width: usize,
    ) -> Result<(), MemoryError> {
        if addresses.len() != values.len() || addresses.len() > 32 || !matches!(width, 2 | 4) {
            return Err(MemoryError::InvalidRange);
        }
        if addresses
            .iter()
            .all(|&address| width as u64 <= ADDRESS_LIMIT - u64::from(address))
        {
            return self.write_stack(addresses, values, width);
        }
        let mut prepared: [Option<CpuSpan>; 32] = std::array::from_fn(|_| None);
        for (index, &address) in addresses.iter().enumerate() {
            let address = GuestAddress(address);
            let span = CpuSpan::new(address, width)?;
            self.cpu_check_span(address, width, Access::Write)?;
            prepared[index] = Some(span);
        }
        let version = self.advance_version()?;
        for (&value, span) in values.iter().zip(prepared.iter().flatten()) {
            self.cpu_copy_in_validated(span, &value.to_le_bytes()[..width], version);
        }
        Ok(())
    }

    pub(crate) fn cpu_write_masked(
        &mut self,
        address: GuestAddress,
        input: &[u8],
        mask: u32,
    ) -> Result<(), MemoryError> {
        if input.len() as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.write_masked(address, input, mask);
        }
        self.cpu_check_masked(address, input.len(), mask, Access::Write)?;
        if mask == 0 {
            return Ok(());
        }
        let version = self.advance_version()?;
        for (index, byte) in input.iter().enumerate() {
            if mask & (1 << index) != 0 {
                self.copy_in_validated(
                    GuestAddress(address.0.wrapping_add(index as u32)),
                    &[*byte],
                    version,
                );
            }
        }
        Ok(())
    }

    pub(super) fn cpu_copy_in_validated(&mut self, span: &CpuSpan, input: &[u8], version: u64) {
        for part in span.parts() {
            self.copy_in_validated(
                part.address,
                &input[part.offset..part.offset + part.length],
                version,
            );
        }
    }
}

#[cfg(test)]
#[path = "write/tests.rs"]
mod tests;
