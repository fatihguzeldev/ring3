use super::*;
use crate::cpu::x86::{
    CpuMemoryPort32, CpuReadPort32, Register32, SegmentAccess,
    simd::ymm::YmmUpperState,
    xstate::{self, XStateOperation, upper},
};

impl NumericBank {
    pub(super) fn xstate(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        operation: XStateOperation,
    ) -> NumericResult<()> {
        let mut general = context.state();
        if operation == XStateOperation::ReadControl {
            let value = xstate::read_control(general.registers[Register32::Ecx.index()])
                .map_err(|_| Failure::Cpu(13, 0))?;
            general.registers[Register32::Eax.index()] = value as u32;
            general.registers[Register32::Edx.index()] = (value >> 32) as u32;
            context.set_state(general);
            return Ok(());
        }
        let (address, save) = match operation {
            XStateOperation::Save { address } => (address, true),
            XStateOperation::Restore { address } => (address, false),
            XStateOperation::ReadControl => unreachable!(),
        };
        let request = upper::requested(
            general.registers[Register32::Eax.index()],
            general.registers[Register32::Edx.index()],
        );
        let offset = memory::offset(address, &general);
        let segment = address.resolved_segment();
        for span in if save {
            upper::access::SAVE_SPANS
        } else {
            upper::access::RESTORE_SPANS
        } {
            if span.selection != 0 && request & u64::from(span.selection) == 0 {
                continue;
            }
            let start = offset
                .checked_add(span.offset)
                .ok_or_else(|| memory::segment_fault(segment))?;
            self.span(
                segment,
                start,
                span.width as usize,
                if save {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
        }
        let linear = self.segments.state.bases[segment.index()].wrapping_add(offset);
        if linear & 63 != 0 {
            return Err(Failure::Cpu(13, 0));
        }
        let mut port = memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?);
        let mut image = [0; upper::IMAGE_SIZE];
        let current_upper = YmmUpperState {
            registers: self.vectors.upper,
        };
        if save {
            port.read_modify(GuestAddress(linear.wrapping_add(512)), &mut image[512..520])
                .map_err(|e| Failure::paging(e, 8))?;
            upper::save(
                request,
                &self.x87,
                &self.vectors.simd,
                &current_upper,
                &mut image,
            );
            let (ranges, count) = upper::access::save_ranges(request);
            let mut spans = [(GuestAddress(0), &[][..]); 4];
            for (index, (start, end)) in ranges[..count].iter().copied().enumerate() {
                spans[index] = (
                    GuestAddress(linear.wrapping_add(start as u32)),
                    &image[start..end],
                );
            }
            port.write_spans(&spans[..count])
                .map_err(|e| Failure::paging(e, upper::IMAGE_SIZE))?;
        } else {
            port.read(GuestAddress(linear.wrapping_add(512)), &mut image[512..536])
                .map_err(|e| Failure::paging(e, 24))?;
            let present = upper::validate_header(image[512..536].try_into().unwrap())
                .map_err(|_| Failure::Cpu(13, 0))?;
            if request & 6 != 0 {
                port.read(GuestAddress(linear.wrapping_add(24)), &mut image[24..28])
                    .map_err(|e| Failure::paging(e, 4))?;
                xstate::validate_mxcsr(u32::from_le_bytes(image[24..28].try_into().unwrap()))
                    .map_err(|_| Failure::Cpu(13, 0))?;
            }
            for (selection, start, end) in [(1, 0, 24), (1, 32, 160), (2, 160, 288), (4, 576, 704)]
            {
                if request & present & selection == 0 {
                    continue;
                }
                port.read(
                    GuestAddress(linear.wrapping_add(start as u32)),
                    &mut image[start..end],
                )
                .map_err(|e| Failure::paging(e, end - start))?;
            }
            let (x87, simd, upper) = upper::restore(
                request,
                &self.x87,
                &self.vectors.simd,
                &current_upper,
                &image,
            )
            .map_err(|_| Failure::Cpu(13, 0))?;
            self.x87 = x87;
            self.vectors = VectorState {
                simd,
                upper: upper.registers,
            };
        }
        Ok(())
    }
}
