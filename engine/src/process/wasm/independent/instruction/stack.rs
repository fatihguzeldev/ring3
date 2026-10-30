use crate::{
    abi::cpu_helper::CpuMemoryError,
    cpu::{
        runtime::numeric::NumericError,
        x86::{
            CpuPagingError32, CpuReadPort32, PagingAccess32, SegmentContext, SegmentRegister,
            StackFrameKind, StackPointerSize, State32, inspect_cpu_paging_access32, ir::Operation,
            read_cpu_paging_access32,
        },
    },
    memory::{AddressSpace, GuestAddress, MemoryError, PagingBacking32},
    process::cpu::stack,
};

struct Port<'a>(&'a mut AddressSpace);

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
        width: usize,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        let width = u32::try_from(width)
            .map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?;
        let backing = match access {
            PagingAccess32::Read => PagingBacking32::Read,
            PagingAccess32::Write => PagingBacking32::Write,
            PagingAccess32::Execute => PagingBacking32::Execute,
        };
        inspect_cpu_paging_access32(self.0, address, width, access, backing)
    }

    fn read_masked(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
        mask: u32,
    ) -> Result<(), CpuPagingError32> {
        if output.is_empty() || output.len() > 32 || output.len() < 32 && mask >> output.len() != 0
        {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        for byte in 0..output.len() {
            if mask & (1 << byte) != 0 {
                self.check(
                    GuestAddress(address.0.wrapping_add(byte as u32)),
                    1,
                    PagingAccess32::Read,
                )?;
            }
        }
        self.0
            .cpu_read_masked(address, output, mask)
            .map_err(CpuPagingError32::Memory)
    }
}

fn segment_failure(error: CpuMemoryError, width: usize) -> NumericError {
    match error {
        CpuMemoryError::Memory(error) => NumericError::Memory {
            error: CpuPagingError32::Memory(error),
            width,
        },
        CpuMemoryError::Paging(error) => NumericError::Memory { error, width },
        CpuMemoryError::GeneralProtection(detail) => NumericError::Cpu { vector: 13, detail },
        CpuMemoryError::StackSegment(detail) => NumericError::Cpu { vector: 12, detail },
        CpuMemoryError::SegmentNotPresent(detail) => NumericError::Cpu { vector: 11, detail },
        _ => NumericError::Infrastructure,
    }
}

pub(super) fn execute(
    memory: &mut AddressSpace,
    segments: &SegmentContext,
    scalar: &mut State32,
    operation: Operation,
) -> Result<bool, NumericError> {
    let (kind, allocation, nesting) = match operation {
        Operation::StackFrame {
            kind,
            allocation,
            nesting,
        } => (kind, allocation, nesting),
        Operation::PopAll => (StackFrameKind::PopAll32, 0, 0),
        Operation::PopAllWord => (StackFrameKind::PopAll16, 0, 0),
        Operation::EnterZero => (StackFrameKind::Enter32, 0, 0),
        Operation::EnterZeroWord => (StackFrameKind::Enter16, 0, 0),
        _ => return Ok(false),
    };
    if nesting > 31
        || !matches!(kind, StackFrameKind::Enter16 | StackFrameKind::Enter32)
            && (allocation != 0 || nesting != 0)
    {
        return Err(NumericError::Infrastructure);
    }
    let width = kind.width() as usize;
    stack::check_segments(segments, kind, allocation, nesting, scalar.registers)
        .map_err(|error| segment_failure(error, width))?;
    let mut port = Port(memory);
    let prepared = stack::prepare(
        &mut port,
        segments.state.bases[SegmentRegister::Ss.index()],
        StackPointerSize::new(segments.caches[SegmentRegister::Ss.index()].default32()),
        kind,
        allocation,
        nesting,
        scalar.registers,
    )
    .map_err(|error| NumericError::Memory { error, width })?;
    if prepared.count != 0 {
        port.0
            .cpu_write_stack(
                &prepared.addresses[..prepared.count],
                &prepared.values[..prepared.count],
                width,
            )
            .map_err(|error| NumericError::Memory {
                error: CpuPagingError32::Memory(error),
                width,
            })?;
    }
    scalar.registers = prepared.registers;
    Ok(true)
}
