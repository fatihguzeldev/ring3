use super::*;
use crate::{
    cpu::x86::{CpuPagingError32, PagingAccess32, SegmentAccess, SegmentRegister, State32, ir::*},
    memory::{Access, CoreWriteOutcome, FaultReason, MemoryError, MemoryFault},
};

#[derive(Debug)]
pub(super) enum Failure {
    Memory(MemoryError, u32),
    Segment(SegmentRegister),
    CpuFault(u32),
    Infrastructure,
}

impl Failure {
    pub(super) fn paging(error: CpuPagingError32, width: u32) -> Self {
        match error {
            CpuPagingError32::Memory(error) => Self::Memory(error, width),
            CpuPagingError32::PageFault(fault) => Self::Memory(
                MemoryError::Fault(MemoryFault {
                    address: fault.linear(),
                    access: match fault.access() {
                        PagingAccess32::Read => Access::Read,
                        PagingAccess32::Write => Access::Write,
                        PagingAccess32::Execute => Access::Execute,
                    },
                    reason: if fault.error_code() & 1 == 0 {
                        FaultReason::Unmapped
                    } else {
                        FaultReason::Permission
                    },
                }),
                width,
            ),
        }
    }

    pub(super) fn publish(self, context: &mut Context32) -> StopReason {
        match self {
            Self::Memory(MemoryError::Fault(fault), width) => {
                let access = match fault.access {
                    Access::Read => 1,
                    Access::Write => 2,
                    Access::Execute => 4,
                };
                let reason = match fault.reason {
                    FaultReason::Unmapped => 1,
                    FaultReason::Permission => 2,
                    FaultReason::AddressOverflow => 3,
                };
                context.set_word(48, fault.address.0);
                context.set_word(52, width | access << 8 | reason << 16);
                context.set_word(116, width << 8 | 14);
                StopReason::MemoryFault
            }
            Self::Segment(segment) => {
                context.set_word(48, 0);
                context.set_word(52, 0);
                context.set_word(
                    116,
                    if segment == SegmentRegister::Ss {
                        12
                    } else {
                        13
                    },
                );
                StopReason::CpuFault
            }
            Self::CpuFault(vector) => {
                context.set_word(48, 0);
                context.set_word(52, 0);
                context.set_word(116, vector);
                StopReason::CpuFault
            }
            Self::Memory(_, _) | Self::Infrastructure => StopReason::Infrastructure,
        }
    }
}

pub(super) struct Port<'a> {
    pub(super) memory: &'a mut AddressSpace,
    pub(super) segments: &'a SegmentContext,
    pub(super) code_written: bool,
}

pub(super) fn offset(address: EffectiveAddress, state: &State32) -> u32 {
    let offset = address
        .displacement
        .wrapping_add(
            address
                .base
                .map_or(0, |register| state.registers[register.index()]),
        )
        .wrapping_add(address.index.map_or(0, |register| {
            state.registers[register.index()].wrapping_mul(u32::from(address.scale))
        }));
    if address.address_size == AddressSize::Bits16 {
        offset & 0xffff
    } else {
        offset
    }
}

impl Port<'_> {
    fn linear(
        &self,
        segment: SegmentRegister,
        offset: u32,
        width: u32,
        access: SegmentAccess,
    ) -> Result<u32, Failure> {
        self.segments.caches[segment.index()]
            .check_span(offset, width, access)
            .map_err(|_| Failure::Segment(segment))?;
        Ok(self.segments.state.bases[segment.index()].wrapping_add(offset))
    }

    pub(super) fn read(
        &self,
        address: EffectiveAddress,
        state: &State32,
        width: u32,
        modify: bool,
    ) -> Result<u32, Failure> {
        let linear = self.linear(
            address.resolved_segment(),
            offset(address, state),
            width,
            if modify {
                SegmentAccess::Write
            } else {
                SegmentAccess::Read
            },
        )?;
        self.memory
            .core_read(linear, width | if modify { 0x100 } else { 0 })
            .map_err(|error| Failure::Memory(error, width))
    }

    /// Check the current element's segment span before either string operand is
    /// accessed. This does not translate, reserve COW, or touch guest memory.
    pub(super) fn check(
        &self,
        address: EffectiveAddress,
        state: &State32,
        width: u32,
        write: bool,
    ) -> Result<(), Failure> {
        self.linear(
            address.resolved_segment(),
            offset(address, state),
            width,
            if write {
                SegmentAccess::Write
            } else {
                SegmentAccess::Read
            },
        )
        .map(|_| ())
    }

    pub(super) fn read_bounds(
        &self,
        address: EffectiveAddress,
        state: &State32,
        word: bool,
    ) -> Result<(i32, i32), Failure> {
        let width = if word { 4 } else { 8 };
        let linear = self.linear(
            address.resolved_segment(),
            offset(address, state),
            width,
            SegmentAccess::Read,
        )?;
        let mut bytes = [0; 8];
        self.memory
            .read(GuestAddress(linear), &mut bytes[..width as usize])
            .map_err(|error| Failure::Memory(error, width))?;
        Ok(if word {
            (
                i32::from(i16::from_le_bytes(bytes[..2].try_into().unwrap())),
                i32::from(i16::from_le_bytes(bytes[2..4].try_into().unwrap())),
            )
        } else {
            (
                i32::from_le_bytes(bytes[..4].try_into().unwrap()),
                i32::from_le_bytes(bytes[4..].try_into().unwrap()),
            )
        })
    }

    pub(super) fn write(
        &mut self,
        address: EffectiveAddress,
        state: &State32,
        width: u32,
        value: u32,
    ) -> Result<(), Failure> {
        let linear = self.linear(
            address.resolved_segment(),
            offset(address, state),
            width,
            SegmentAccess::Write,
        )?;
        self.store(linear, width, value)
    }

    pub(super) fn stack_read(&self, offset: u32, width: u32) -> Result<u32, Failure> {
        let linear = self.linear(SegmentRegister::Ss, offset, width, SegmentAccess::Read)?;
        self.memory
            .core_read(linear, width)
            .map_err(|error| Failure::Memory(error, width))
    }

    pub(super) fn stack_write(
        &mut self,
        offset: u32,
        width: u32,
        value: u32,
    ) -> Result<(), Failure> {
        let linear = self.linear(SegmentRegister::Ss, offset, width, SegmentAccess::Write)?;
        self.store(linear, width, value)
    }

    fn store(&mut self, linear: u32, width: u32, value: u32) -> Result<(), Failure> {
        let outcome = self
            .memory
            .core_write(linear, width, value)
            .map_err(|error| Failure::Memory(error, width))?;
        self.code_written |= outcome == CoreWriteOutcome::CodeInvalidated;
        Ok(())
    }
}
