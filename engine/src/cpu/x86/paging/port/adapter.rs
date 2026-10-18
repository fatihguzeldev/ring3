use super::{CpuMemoryPort32, CpuReadPort32};
use crate::{
    cpu::x86::{
        CpuPagingError32, PagingAccess32, inspect_cpu_paging_access32, read_cpu_paging_access32,
    },
    memory::{AddressSpace, GuestAddress, MemoryError, PagingBacking32},
};

pub(crate) struct DefaultCpuReadPort32<'a> {
    memory: &'a AddressSpace,
}
pub(crate) struct DefaultCpuMemoryPort32<'a> {
    memory: &'a mut AddressSpace,
}
impl<'a> DefaultCpuReadPort32<'a> {
    pub(crate) fn new(memory: &'a AddressSpace) -> Self {
        Self { memory }
    }
}
impl<'a> DefaultCpuMemoryPort32<'a> {
    pub(crate) fn new(memory: &'a mut AddressSpace) -> Self {
        Self { memory }
    }
}
fn preflight(
    memory: &AddressSpace,
    address: GuestAddress,
    length: usize,
    access: PagingAccess32,
    backing: PagingBacking32,
) -> Result<(), CpuPagingError32> {
    let length =
        u32::try_from(length).map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?;
    inspect_cpu_paging_access32(memory, address, length, access, backing)
}
fn backing(access: PagingAccess32) -> PagingBacking32 {
    match access {
        PagingAccess32::Read => PagingBacking32::Read,
        PagingAccess32::Write => PagingBacking32::Write,
        PagingAccess32::Execute => PagingBacking32::Execute,
    }
}
fn masked_preflight(
    memory: &AddressSpace,
    address: GuestAddress,
    length: usize,
    mask: u32,
    access: PagingAccess32,
) -> Result<(), CpuPagingError32> {
    if length == 0 || length > 32 || length < 32 && mask >> length != 0 {
        return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
    }
    for offset in 0..length {
        if mask & (1 << offset) != 0 {
            preflight(
                memory,
                GuestAddress(address.0.wrapping_add(offset as u32)),
                1,
                access,
                backing(access),
            )?;
        }
    }
    Ok(())
}
impl CpuReadPort32 for DefaultCpuReadPort32<'_> {
    fn read(&mut self, address: GuestAddress, output: &mut [u8]) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(self.memory, address, output, PagingAccess32::Read)
    }
    fn read_modify(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(self.memory, address, output, PagingAccess32::Write)
    }
    fn check(
        &mut self,
        address: GuestAddress,
        length: usize,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        preflight(self.memory, address, length, access, backing(access))
    }
    fn read_masked(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        masked_preflight(
            self.memory,
            address,
            output.len(),
            mask,
            PagingAccess32::Read,
        )?;
        self.memory
            .cpu_read_masked(address, output, mask)
            .map_err(CpuPagingError32::Memory)
    }
}
impl CpuReadPort32 for DefaultCpuMemoryPort32<'_> {
    fn read(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        DefaultCpuReadPort32::new(self.memory).read(a, o)
    }
    fn read_modify(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        DefaultCpuReadPort32::new(self.memory).read_modify(a, o)
    }
    fn check(
        &mut self,
        a: GuestAddress,
        n: usize,
        x: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        DefaultCpuReadPort32::new(self.memory).check(a, n, x)
    }
    fn read_masked(
        &mut self,
        a: GuestAddress,
        o: &mut [u8],
        m: u32,
    ) -> Result<(), CpuPagingError32> {
        DefaultCpuReadPort32::new(self.memory).read_masked(a, o, m)
    }
}
impl CpuMemoryPort32 for DefaultCpuMemoryPort32<'_> {
    fn write(&mut self, a: GuestAddress, i: &[u8]) -> Result<(), CpuPagingError32> {
        preflight(
            self.memory,
            a,
            i.len(),
            PagingAccess32::Write,
            PagingBacking32::Write,
        )?;
        self.memory
            .cpu_write(a, i)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_spans(&mut self, spans: &[(GuestAddress, &[u8])]) -> Result<(), CpuPagingError32> {
        if spans.len() > 5 {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for &(a, i) in spans {
            preflight(
                self.memory,
                a,
                i.len(),
                PagingAccess32::Write,
                PagingBacking32::Write,
            )?;
        }
        self.memory
            .cpu_write_spans(spans)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_stack(
        &mut self,
        addresses: &[u32],
        values: &[u32],
        width: usize,
    ) -> Result<(), CpuPagingError32> {
        if addresses.len() != values.len() || addresses.len() > 32 || !matches!(width, 2 | 4) {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for &a in addresses {
            preflight(
                self.memory,
                GuestAddress(a),
                width,
                PagingAccess32::Write,
                PagingBacking32::Write,
            )?;
        }
        self.memory
            .cpu_write_stack(addresses, values, width)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_masked(
        &mut self,
        a: GuestAddress,
        i: &[u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        masked_preflight(self.memory, a, i.len(), mask, PagingAccess32::Write)?;
        self.memory
            .cpu_write_masked(a, i, mask)
            .map_err(CpuPagingError32::Memory)
    }
}
