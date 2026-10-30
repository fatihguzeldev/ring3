use super::*;
use crate::{
    cpu::x86::avx::{
        AvxOperation, GatherFaultCause, MaskedError, MemoryEffect,
        dispatch::{self, DispatchError},
    },
    memory::{Access, MemoryError},
};

impl NumericState {
    pub(super) fn avx(
        &mut self,
        memory: &mut AddressSpace,
        scalar: &mut State32,
        segments: &SegmentContext,
        operation: AvxOperation,
    ) -> Result<(), NumericError> {
        let instruction = operation.instruction();
        if let MemoryEffect::Gather(gather) = instruction.memory() {
            let address = operation
                .gather_address()
                .ok_or(NumericError::Infrastructure)?;
            let inputs = gather.capture(&self.vectors);
            let selected = gather.selected(&inputs.mask);
            let offset = address.displacement().wrapping_add(
                address
                    .base()
                    .map_or(0, |register| scalar.registers[register.index()]),
            );
            let result = gather
                .evaluate_with_port(
                    inputs,
                    address.resolved_segment(),
                    offset,
                    (selected != 0).then_some(segments),
                    &mut memory::Port(memory),
                )
                .map_err(|_| NumericError::Infrastructure)?;
            for (register, value) in [
                (gather.destination(), result.destination),
                (gather.mask(), result.mask),
            ] {
                self.vectors.simd.xmm[register.index()].copy_from_slice(&value[..16]);
                self.vectors.upper[register.index()].copy_from_slice(&value[16..]);
            }
            if let Some(fault) = result.fault {
                let width = gather.geometry().data_bytes();
                return Err(match fault.cause {
                    GatherFaultCause::Segment { segment, .. } => NumericError::segment(segment),
                    GatherFaultCause::Memory(fault) => NumericError::memory(
                        CpuPagingError32::Memory(MemoryError::Fault(fault)),
                        width,
                    ),
                    GatherFaultCause::Paging(error) => NumericError::memory(error, width),
                });
            }
            return Ok(());
        }
        if let MemoryEffect::Selected(masked) = instruction.memory() {
            let address = operation.address().ok_or(NumericError::Infrastructure)?;
            let before = self.vectors;
            let result = self.vectors.execute_masked_with_port(
                masked,
                segments,
                address.resolved_segment(),
                offset(address, scalar),
                &mut memory::Port(memory),
            );
            if let Err(error) = result {
                self.vectors = before;
                return Err(match error {
                    MaskedError::InvalidState => NumericError::Infrastructure,
                    MaskedError::Segment { segment, .. } => NumericError::segment(segment),
                    MaskedError::Memory(error) => NumericError::memory(
                        CpuPagingError32::Memory(error),
                        masked.width().bytes(),
                    ),
                    MaskedError::Paging(error) => {
                        NumericError::memory(error, masked.width().bytes())
                    }
                });
            }
            return Ok(());
        }
        let mut payload = [0; 32];
        let mut linear = 0;
        let mut length = 0;
        let mut store = false;
        if let MemoryEffect::Full(requirement) = instruction.memory() {
            length = usize::from(requirement.bytes);
            store = requirement.access == Access::Write;
            linear = operand(
                segments,
                operation.address().ok_or(NumericError::Infrastructure)?,
                scalar,
                length,
                if store {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
            if linear & (u32::from(requirement.alignment) - 1) != 0 {
                return Err(NumericError::Cpu {
                    vector: 13,
                    detail: 0,
                });
            }
            let mut port = memory::Port(memory);
            if store {
                port.check(GuestAddress(linear), length, PagingAccess32::Write)
                    .map_err(|error| NumericError::memory(error, length))?;
            } else {
                port.read(GuestAddress(linear), &mut payload[..length])
                    .map_err(|error| NumericError::memory(error, length))?;
            }
        }
        let before = self.vectors;
        let mut general = *scalar;
        if let Err(error) = dispatch::execute(
            instruction,
            &mut self.vectors,
            &mut general,
            linear,
            &mut payload[..length],
        ) {
            let mxcsr = self.vectors.simd.mxcsr;
            self.vectors = before;
            return Err(match error {
                DispatchError::InvalidState => NumericError::Infrastructure,
                DispatchError::GeneralProtection => NumericError::Cpu {
                    vector: 13,
                    detail: 0,
                },
                DispatchError::FloatingPoint(_) => {
                    self.vectors.simd.mxcsr = mxcsr;
                    NumericError::Cpu {
                        vector: 19,
                        detail: mxcsr,
                    }
                }
            });
        }
        if store {
            if let Err(error) = memory::Port(memory).write(GuestAddress(linear), &payload[..length])
            {
                self.vectors = before;
                return Err(NumericError::memory(error, length));
            }
        }
        *scalar = general;
        Ok(())
    }
}
