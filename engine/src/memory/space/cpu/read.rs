use super::{ADDRESS_LIMIT, Access, AddressSpace, CpuSpan, GuestAddress, MemoryError};

impl AddressSpace {
    pub(crate) fn cpu_read(
        &self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), MemoryError> {
        if output.len() as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.read(address, output);
        }
        let span = CpuSpan::new(address, output.len())?;
        // complete both ordered probes before exposing any prefix to the caller.
        for part in span.parts() {
            self.cpu_check_span(part.address, part.length, Access::Read)?;
        }
        for part in span.parts() {
            self.read(
                part.address,
                &mut output[part.offset..part.offset + part.length],
            )?;
        }
        Ok(())
    }
}

impl AddressSpace {
    pub(crate) fn cpu_read_masked(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        mask: u32,
    ) -> Result<(), MemoryError> {
        if output.len() as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.read_masked(address, output, mask);
        }
        self.cpu_check_masked(address, output.len(), mask, Access::Read)?;
        for (index, byte) in output.iter_mut().enumerate() {
            if mask & (1 << index) != 0 {
                self.read(
                    GuestAddress(address.0.wrapping_add(index as u32)),
                    std::slice::from_mut(byte),
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
