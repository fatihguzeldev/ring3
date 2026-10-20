use super::*;
use crate::{
    cpu::x86::{
        CpuMemoryPort32, CpuReadPort32, PagingAccess32, SegmentAccess, SegmentRegister, State32,
        inspect_cpu_paging_access32,
        ir::{AddressSize, EffectiveAddress},
        read_cpu_paging_access32,
    },
    memory::{AddressSpace, PagingBacking32},
};

pub(super) struct Port<'a>(pub(super) &'a mut AddressSpace);

fn probe(
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

impl CpuReadPort32 for Port<'_> {
    fn read(&mut self, address: GuestAddress, output: &mut [u8]) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(self.0, address, output, PagingAccess32::Read)
    }
    fn read_modify(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), CpuPagingError32> {
        read_cpu_paging_access32(self.0, address, output, PagingAccess32::Write)
    }
    fn check(
        &mut self,
        address: GuestAddress,
        length: usize,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        probe(self.0, address, length, access, backing(access))
    }
    fn read_masked(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        self.masked_check(address, output.len(), mask, PagingAccess32::Read)?;
        self.0
            .cpu_read_masked(address, output, mask)
            .map_err(CpuPagingError32::Memory)
    }
}

impl Port<'_> {
    fn masked_check(
        &self,
        address: GuestAddress,
        length: usize,
        mask: u32,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        if length == 0 || length > 32 || length < 32 && mask >> length != 0 {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for byte in 0..length {
            if mask & (1 << byte) != 0 {
                probe(
                    self.0,
                    GuestAddress(address.0.wrapping_add(byte as u32)),
                    1,
                    access,
                    backing(access),
                )?;
            }
        }
        Ok(())
    }
}

impl CpuMemoryPort32 for Port<'_> {
    fn write(&mut self, address: GuestAddress, input: &[u8]) -> Result<(), CpuPagingError32> {
        self.check(address, input.len(), PagingAccess32::Write)?;
        self.0
            .cpu_write(address, input)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_spans(&mut self, spans: &[(GuestAddress, &[u8])]) -> Result<(), CpuPagingError32> {
        for &(address, input) in spans {
            self.check(address, input.len(), PagingAccess32::Write)?;
        }
        self.0
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
        for &address in addresses {
            self.check(GuestAddress(address), width, PagingAccess32::Write)?;
        }
        self.0
            .cpu_write_stack(addresses, values, width)
            .map_err(CpuPagingError32::Memory)
    }
    fn write_masked(
        &mut self,
        address: GuestAddress,
        input: &[u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        self.masked_check(address, input.len(), mask, PagingAccess32::Write)?;
        self.0
            .cpu_write_masked(address, input, mask)
            .map_err(CpuPagingError32::Memory)
    }
}

pub(super) fn offset(address: EffectiveAddress, state: &State32) -> u32 {
    let value = address
        .displacement
        .wrapping_add(address.base.map_or(0, |r| state.registers[r.index()]))
        .wrapping_add(address.index.map_or(0, |r| {
            state.registers[r.index()].wrapping_mul(u32::from(address.scale))
        }));
    match address.address_size {
        AddressSize::Bits16 => value & 0xffff,
        AddressSize::Bits32 => value,
    }
}

pub(super) fn segment_fault(segment: SegmentRegister) -> Failure {
    Failure::Cpu(
        if segment == SegmentRegister::Ss {
            12
        } else {
            13
        },
        0,
    )
}

impl NumericBank {
    pub(super) fn span(
        &self,
        segment: SegmentRegister,
        offset: u32,
        width: usize,
        access: SegmentAccess,
    ) -> NumericResult<u32> {
        self.segments.caches[segment.index()]
            .check_span(offset, width as u32, access)
            .map_err(|_| segment_fault(segment))?;
        Ok(self.segments.state.bases[segment.index()].wrapping_add(offset))
    }

    pub(super) fn operand(
        &self,
        address: EffectiveAddress,
        state: &State32,
        width: usize,
        access: SegmentAccess,
    ) -> NumericResult<u32> {
        self.span(
            address.resolved_segment(),
            offset(address, state),
            width,
            access,
        )
    }
}
