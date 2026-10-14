use super::CpuPagingError32;
use crate::{cpu::x86::PagingAccess32, memory::GuestAddress};

pub(crate) trait CpuReadPort32 {
    fn read(&mut self, address: GuestAddress, output: &mut [u8]) -> Result<(), CpuPagingError32>;
    fn read_modify(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), CpuPagingError32>;
    fn check(
        &mut self,
        address: GuestAddress,
        length: usize,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32>;
    fn read_masked(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32>;
}
pub(crate) trait CpuMemoryPort32: CpuReadPort32 {
    fn write(&mut self, address: GuestAddress, input: &[u8]) -> Result<(), CpuPagingError32>;
    fn write_spans(&mut self, spans: &[(GuestAddress, &[u8])]) -> Result<(), CpuPagingError32>;
    fn write_stack(
        &mut self,
        addresses: &[u32],
        values: &[u32],
        width: usize,
    ) -> Result<(), CpuPagingError32>;
    fn write_masked(
        &mut self,
        address: GuestAddress,
        input: &[u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32>;
}

#[cfg(test)]
mod adapter;
#[cfg(test)]
pub(crate) use adapter::{DefaultCpuMemoryPort32, DefaultCpuReadPort32};

#[cfg(test)]
mod tests;
