use super::*;
use crate::cpu::x86::{
    Register32,
    simd::ymm::YmmUpperState,
    xstate::{self, XStateOperation, upper},
};

impl NumericState {
    pub(super) fn xstate(
        &mut self,
        memory: &mut AddressSpace,
        scalar: &mut State32,
        segments: &SegmentContext,
        operation: XStateOperation,
    ) -> Result<(), NumericError> {
        if operation == XStateOperation::ReadControl {
            let value =
                xstate::read_control(scalar.registers[Register32::Ecx.index()]).map_err(|_| {
                    NumericError::Cpu {
                        vector: 13,
                        detail: 0,
                    }
                })?;
            scalar.registers[Register32::Eax.index()] = value as u32;
            scalar.registers[Register32::Edx.index()] = (value >> 32) as u32;
            return Ok(());
        }
        let (address, save) = match operation {
            XStateOperation::Save { address } => (address, true),
            XStateOperation::Restore { address } => (address, false),
            XStateOperation::ReadControl => unreachable!(),
        };
        let request = upper::requested(
            scalar.registers[Register32::Eax.index()],
            scalar.registers[Register32::Edx.index()],
        );
        let offset = offset(address, scalar);
        let segment = address.resolved_segment();
        for range in if save {
            upper::access::SAVE_SPANS
        } else {
            upper::access::RESTORE_SPANS
        } {
            if range.selection != 0 && request & u64::from(range.selection) == 0 {
                continue;
            }
            let start = offset
                .checked_add(range.offset)
                .ok_or_else(|| NumericError::segment(segment))?;
            span(
                segments,
                segment,
                start,
                range.width as usize,
                if save {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
        }
        let linear = segments.state.bases[segment.index()].wrapping_add(offset);
        if linear & 63 != 0 {
            return Err(NumericError::Cpu {
                vector: 13,
                detail: 0,
            });
        }
        let mut port = memory::Port(memory);
        let mut image = [0; upper::IMAGE_SIZE];
        let current_upper = YmmUpperState {
            registers: self.vectors.upper,
        };
        if save {
            port.read_modify(GuestAddress(linear.wrapping_add(512)), &mut image[512..520])
                .map_err(|error| NumericError::memory(error, 8))?;
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
                .map_err(|error| NumericError::memory(error, upper::IMAGE_SIZE))?;
        } else {
            port.read(GuestAddress(linear.wrapping_add(512)), &mut image[512..536])
                .map_err(|error| NumericError::memory(error, 24))?;
            let present =
                upper::validate_header(image[512..536].try_into().unwrap()).map_err(|_| {
                    NumericError::Cpu {
                        vector: 13,
                        detail: 0,
                    }
                })?;
            if request & 6 != 0 {
                port.read(GuestAddress(linear.wrapping_add(24)), &mut image[24..28])
                    .map_err(|error| NumericError::memory(error, 4))?;
                xstate::validate_mxcsr(u32::from_le_bytes(image[24..28].try_into().unwrap()))
                    .map_err(|_| NumericError::Cpu {
                        vector: 13,
                        detail: 0,
                    })?;
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
                .map_err(|error| NumericError::memory(error, end - start))?;
            }
            let (x87, simd, upper) = upper::restore(
                request,
                &self.x87,
                &self.vectors.simd,
                &current_upper,
                &image,
            )
            .map_err(|_| NumericError::Cpu {
                vector: 13,
                detail: 0,
            })?;
            self.x87 = x87;
            self.vectors = VectorState {
                simd,
                upper: upper.registers,
            };
        }
        Ok(())
    }
}
