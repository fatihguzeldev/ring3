use super::*;
use crate::cpu::x86::{
    CpuMemoryPort32, CpuReadPort32, PagingAccess32, SegmentAccess,
    avx::{
        AvxOperation, AvxSemantic, GatherFaultCause, MaskedError, MemoryEffect,
        dispatch::{self, DispatchError},
    },
};

impl NumericBank {
    pub(super) fn avx(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        operation: AvxOperation,
    ) -> NumericResult<()> {
        let instruction = operation.instruction();
        let mut general = context.state();
        if let MemoryEffect::Gather(gather) = instruction.memory() {
            let address = operation.gather_address().ok_or(Failure::Infrastructure)?;
            let inputs = gather.capture(&self.vectors);
            let selected = gather.selected(&inputs.mask);
            let offset = address
                .displacement()
                .wrapping_add(address.base().map_or(0, |r| general.registers[r.index()]));
            let outcome = gather
                .evaluate_with_port(
                    inputs,
                    address.resolved_segment(),
                    offset,
                    (selected != 0).then_some(&self.segments),
                    &mut memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?),
                )
                .map_err(|_| Failure::Infrastructure)?;
            for (register, value) in [
                (gather.destination(), outcome.destination),
                (gather.mask(), outcome.mask),
            ] {
                self.vectors.simd.xmm[register.index()].copy_from_slice(&value[..16]);
                self.vectors.upper[register.index()].copy_from_slice(&value[16..]);
            }
            if let Some(fault) = outcome.fault {
                let width = gather.geometry().data_bytes();
                return Err(match fault.cause {
                    GatherFaultCause::Segment { segment, .. } => memory::segment_fault(segment),
                    GatherFaultCause::Memory(fault) => {
                        Failure::Memory(CpuMemoryError::Memory(MemoryError::Fault(fault)), width)
                    }
                    GatherFaultCause::Paging(error) => Failure::paging(error, width),
                });
            }
            return Ok(());
        }
        if let MemoryEffect::Selected(masked) = instruction.memory() {
            let address = operation.address().ok_or(Failure::Infrastructure)?;
            let width = masked.width().bytes();
            let before = self.vectors;
            let result = self.vectors.execute_masked_with_port(
                masked,
                &self.segments,
                address.resolved_segment(),
                memory::offset(address, &general),
                &mut memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?),
            );
            if let Err(error) = result {
                self.vectors = before;
                return Err(match error {
                    MaskedError::InvalidState => Failure::Infrastructure,
                    MaskedError::Segment { segment, .. } => memory::segment_fault(segment),
                    MaskedError::Memory(error) => {
                        Failure::Memory(CpuMemoryError::Memory(error), width)
                    }
                    MaskedError::Paging(error) => Failure::paging(error, width),
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
            let address = operation.address().ok_or(Failure::Infrastructure)?;
            linear = self.operand(
                address,
                &general,
                length,
                if store {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
            if linear & (u32::from(requirement.alignment) - 1) != 0 {
                return Err(Failure::Cpu(13, 0));
            }
            let mut port = memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?);
            if store && matches!(instruction.semantic(), AvxSemantic::F16c(_)) {
                port.check(GuestAddress(linear), length, PagingAccess32::Write)
                    .map_err(|e| Failure::paging(e, length))?;
            }
            if !store {
                port.read(GuestAddress(linear), &mut payload[..length])
                    .map_err(|e| Failure::paging(e, length))?;
            }
        }
        let before = self.vectors;
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
                DispatchError::InvalidState => Failure::Infrastructure,
                DispatchError::GeneralProtection => Failure::Cpu(13, 0),
                DispatchError::FloatingPoint(_) => {
                    self.vectors.simd.mxcsr = mxcsr;
                    Failure::Cpu(19, mxcsr)
                }
            });
        }
        if store {
            if let Err(error) =
                memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?)
                    .write(GuestAddress(linear), &payload[..length])
            {
                self.vectors = before;
                return Err(Failure::paging(error, length));
            }
        }
        context.set_state(general);
        Ok(())
    }
}
