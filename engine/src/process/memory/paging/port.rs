use super::*;
use crate::cpu::x86::{CpuMemoryPort32, CpuReadPort32};

pub(in crate::process) struct EngineCpuMemoryPort32<'a> {
    engine: &'a mut EngineInstance,
}
#[cfg(test)]
pub(in crate::process) struct InspectCpuMemoryPort32<'a> {
    engine: &'a EngineInstance,
}
impl EngineInstance {
    pub(in crate::process) fn cpu_memory_port32(&mut self) -> EngineCpuMemoryPort32<'_> {
        EngineCpuMemoryPort32 { engine: self }
    }
    #[cfg(test)]
    pub(in crate::process) fn inspect_cpu_memory_port32(&self) -> InspectCpuMemoryPort32<'_> {
        InspectCpuMemoryPort32 { engine: self }
    }
}
fn width(length: usize) -> Result<u32, CpuPagingError32> {
    u32::try_from(length).map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))
}
fn backing(access: PagingAccess32) -> PagingBacking32 {
    match access {
        PagingAccess32::Read => PagingBacking32::Read,
        PagingAccess32::Write => PagingBacking32::Write,
        PagingAccess32::Execute => PagingBacking32::Execute,
    }
}
#[cfg(test)]
impl CpuReadPort32 for InspectCpuMemoryPort32<'_> {
    fn read(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(
            self.engine.memory.as_ref().unwrap(),
            a,
            o,
            PagingAccess32::Read,
        )
    }
    fn read_modify(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(
            self.engine.memory.as_ref().unwrap(),
            a,
            o,
            PagingAccess32::Write,
        )
    }
    fn check(
        &mut self,
        a: GuestAddress,
        n: usize,
        x: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        self.engine.inspect_cpu_paging32(a, width(n)?, x)
    }
    fn read_masked(
        &mut self,
        a: GuestAddress,
        o: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        if o.is_empty() || o.len() > 32 || o.len() < 32 && mask >> o.len() != 0 {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for offset in 0..o.len() {
            if mask & (1 << offset) != 0 {
                self.engine.inspect_cpu_paging32(
                    GuestAddress(a.0.wrapping_add(offset as u32)),
                    1,
                    PagingAccess32::Read,
                )?;
            }
        }
        self.engine
            .memory
            .as_ref()
            .unwrap()
            .cpu_read_masked(a, o, mask)
            .map_err(CpuPagingError32::Memory)
    }
}
impl CpuReadPort32 for EngineCpuMemoryPort32<'_> {
    fn read(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        self.engine.read_cpu_paging32(a, o, PagingAccess32::Read)
    }
    fn read_modify(&mut self, a: GuestAddress, o: &mut [u8]) -> Result<(), CpuPagingError32> {
        self.engine.read_cpu_paging32(a, o, PagingAccess32::Write)
    }
    fn check(
        &mut self,
        a: GuestAddress,
        n: usize,
        x: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        self.engine
            .preflight_cpu_paging_backing32(a, width(n)?, x, backing(x))
    }
    fn read_masked(
        &mut self,
        a: GuestAddress,
        o: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        self.engine.preflight_cpu_paging_masked32(
            a,
            width(o.len())?,
            mask,
            PagingAccess32::Read,
        )?;
        self.engine
            .memory
            .as_ref()
            .unwrap()
            .cpu_read_masked(a, o, mask)
            .map_err(CpuPagingError32::Memory)
    }
}
impl CpuMemoryPort32 for EngineCpuMemoryPort32<'_> {
    fn write(&mut self, a: GuestAddress, i: &[u8]) -> Result<(), CpuPagingError32> {
        self.engine
            .preflight_cpu_paging32(a, width(i.len())?, PagingAccess32::Write)?;
        self.engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(a, i)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_spans(&mut self, spans: &[(GuestAddress, &[u8])]) -> Result<(), CpuPagingError32> {
        if spans.len() > 5 {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for &(a, i) in spans {
            self.engine
                .preflight_cpu_paging32(a, width(i.len())?, PagingAccess32::Write)?;
        }
        self.engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write_spans(spans)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_stack(
        &mut self,
        addresses: &[u32],
        values: &[u32],
        n: usize,
    ) -> Result<(), CpuPagingError32> {
        if addresses.len() != values.len() || addresses.len() > 32 || !matches!(n, 2 | 4) {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for &a in addresses {
            self.engine
                .preflight_cpu_paging32(GuestAddress(a), n as u32, PagingAccess32::Write)?;
        }
        self.engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write_stack(addresses, values, n)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_masked(
        &mut self,
        a: GuestAddress,
        i: &[u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        self.engine.preflight_cpu_paging_masked32(
            a,
            width(i.len())?,
            mask,
            PagingAccess32::Write,
        )?;
        self.engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write_masked(a, i, mask)
            .map_err(CpuPagingError32::Memory)
    }
}

#[cfg(test)]
mod tests;
