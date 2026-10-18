use super::*;

impl AddressSpace {
    pub(crate) fn cpu_paging_read32(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        access: Access,
    ) -> Result<(), PagingCheckError32> {
        self.cpu_paging_check32(address, output.len(), access, PagingBacking32::Read)?;
        // the immutable borrow keeps every checked mapping and permission current
        // until the complete span has been copied; no authority escapes this call.
        let mut copied = 0;
        while copied < output.len() {
            let current = address.0.wrapping_add(copied as u32) as usize;
            let page = current / PAGE_SIZE as usize;
            let within = current % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(output.len() - copied);
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            output[copied..copied + count].copy_from_slice(&self.backing[offset..offset + count]);
            copied += count;
        }
        Ok(())
    }
}
