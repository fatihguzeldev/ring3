use super::{part_address, read_part};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, X87_OFFSET, YMM_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result_v8},
        memory_helper::HELPER_SIZE,
        x86::{SIMD_SIZE, X87_SIZE, YMM_SIZE, decode_ymm, encode_simd, encode_x87, encode_ymm},
    },
    cpu::x86::{
        CpuMemoryPort32, CpuReadPort32, SegmentAccess, SegmentRegister, X87State,
        simd::{SimdState, ymm::YmmUpperState},
        xstate::{
            self,
            upper::{self, IMAGE_SIZE},
        },
    },
    process::{EngineInstance, HostError, StoreCompletion, cpu::segments::check_cached_span},
};

impl EngineInstance {
    pub fn cpu_xsave_avx(
        &mut self,
        segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.xsave_avx_bound(None, segment, offset, low, high)
    }
    pub fn cpu_xsave_avx_resident(
        &mut self,
        key: u64,
        id: u64,
        segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.xsave_avx_bound(Some(id), segment, offset, low, high)
    }
    pub fn cpu_xrstor_avx(
        &mut self,
        segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.xrstor_avx_bound(segment, offset, low, high)
    }
    pub fn cpu_xrstor_avx_resident(
        &mut self,
        key: u64,
        id: u64,
        segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.xrstor_avx_bound(segment, offset, low, high)
    }
    fn upper_current(&self) -> Result<(X87State, SimdState, YmmUpperState), HostError> {
        let (x87, simd) = self.xstate_current()?;
        let upper = decode_ymm(&self.arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        Ok((x87, simd, upper))
    }
    fn upper_address(
        context: &crate::cpu::x86::SegmentContext,
        segment: SegmentRegister,
        offset: u32,
        request: u64,
        save: bool,
    ) -> Result<u32, CpuMemoryError> {
        let spans = if save {
            upper::access::SAVE_SPANS
        } else {
            upper::access::RESTORE_SPANS
        };
        let access = if save {
            SegmentAccess::Write
        } else {
            SegmentAccess::Read
        };
        for span in spans {
            if span.selection != 0 && request & u64::from(span.selection) == 0 {
                continue;
            }
            let fault = if segment == SegmentRegister::Ss {
                CpuMemoryError::StackSegment(0)
            } else {
                CpuMemoryError::GeneralProtection(0)
            };
            let start = offset.checked_add(span.offset).ok_or(fault)?;
            check_cached_span(context, segment, start, span.width, access)?;
        }
        Ok(context.state.bases[segment.index()].wrapping_add(offset))
    }
    fn xsave_avx_bound(
        &mut self,
        resident: Option<u64>,
        raw_segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        let segment = segment(raw_segment)?;
        let (x87, simd, upper_state) = self.upper_current()?;
        let context = self.operand_segment_context()?;
        let request = upper::requested(low, high);
        let address = match Self::upper_address(&context, segment, offset, request, true) {
            Ok(address) if address & 63 == 0 => address,
            Ok(_) => {
                self.upper_packet(Err(CpuMemoryError::GeneralProtection(0)))?;
                return Ok(StoreCompletion::Complete);
            }
            Err(error) => {
                self.upper_packet(Err(error))?;
                return Ok(StoreCompletion::Complete);
            }
        };
        let mut success = [0; HELPER_SIZE];
        encode_cpu_result_v8(IMAGE_SIZE as u32, 0, Ok(()), &mut success)
            .map_err(|_| HostError::Infrastructure)?;
        let version = self.memory.as_ref().unwrap().code_validation_version();
        let result = save(
            &mut self.cpu_memory_port32(),
            address,
            request,
            &x87,
            &simd,
            &upper_state,
        );
        let changed =
            result.is_ok() && version != self.memory.as_ref().unwrap().code_validation_version();
        match result {
            Ok(()) => {
                self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&success)
            }
            Err(error) => self.upper_packet(Err(error))?,
        }
        self.cpu_store_currentness(changed, resident)
    }
    fn xrstor_avx_bound(
        &mut self,
        raw_segment: u32,
        offset: u32,
        low: u32,
        high: u32,
    ) -> Result<(), HostError> {
        let segment = segment(raw_segment)?;
        let (current_x87, current_simd, current_upper) = self.upper_current()?;
        let context = self.operand_segment_context()?;
        let request = upper::requested(low, high);
        let address = match Self::upper_address(&context, segment, offset, request, false) {
            Ok(address) if address & 63 == 0 => address,
            Ok(_) => return self.upper_packet(Err(CpuMemoryError::GeneralProtection(0))),
            Err(error) => return self.upper_packet(Err(error)),
        };
        let (x87, simd, upper_state) = match restore(
            &mut self.cpu_memory_port32(),
            address,
            request,
            &current_x87,
            &current_simd,
            &current_upper,
        ) {
            Ok(states) => states,
            Err(error) => return self.upper_packet(Err(error)),
        };
        let mut x87_record = [0; X87_SIZE];
        let mut simd_record = [0; SIMD_SIZE];
        let mut upper_record = [0; YMM_SIZE];
        let mut packet = [0; HELPER_SIZE];
        encode_x87(&x87, &mut x87_record).map_err(|_| HostError::Infrastructure)?;
        encode_simd(&simd, &mut simd_record).map_err(|_| HostError::Infrastructure)?;
        encode_ymm(&upper_state, &mut upper_record).map_err(|_| HostError::Infrastructure)?;
        encode_cpu_result_v8(IMAGE_SIZE as u32, 0, Ok(()), &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        if request & 1 != 0 {
            self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&x87_record);
        }
        if request & 6 != 0 {
            self.arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&simd_record);
        }
        if request & 4 != 0 {
            self.arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE].copy_from_slice(&upper_record);
        }
        self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
        Ok(())
    }
    fn upper_packet(&mut self, result: Result<(), CpuMemoryError>) -> Result<(), HostError> {
        encode_cpu_result_v8(
            IMAGE_SIZE as u32,
            0,
            result,
            &mut self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}
fn segment(raw: u32) -> Result<SegmentRegister, HostError> {
    [
        SegmentRegister::Es,
        SegmentRegister::Cs,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ]
    .get(raw as usize)
    .copied()
    .ok_or(HostError::InvalidRequest)
}
fn save(
    memory: &mut impl CpuMemoryPort32,
    address: u32,
    request: u64,
    x87: &X87State,
    simd: &SimdState,
    upper_state: &YmmUpperState,
) -> Result<(), CpuMemoryError> {
    let mut image = [0; IMAGE_SIZE];
    memory
        .read_modify(part_address(address, 512), &mut image[512..520])
        .map_err(CpuMemoryError::from)?;
    upper::save(request, x87, simd, upper_state, &mut image);
    let (ranges, count) = upper::access::save_ranges(request);
    let mut spans = [(crate::memory::GuestAddress(0), &[][..]); 4];
    for (index, (start, end)) in ranges[..count].iter().copied().enumerate() {
        spans[index] = (part_address(address, start as u32), &image[start..end]);
    }
    memory
        .write_spans(&spans[..count])
        .map_err(CpuMemoryError::from)
}
fn restore(
    memory: &mut impl CpuReadPort32,
    address: u32,
    request: u64,
    x87: &X87State,
    simd: &SimdState,
    upper_state: &YmmUpperState,
) -> Result<(X87State, SimdState, YmmUpperState), CpuMemoryError> {
    let mut image = [0; IMAGE_SIZE];
    read_part(memory, address, 512, &mut image[512..536])?;
    let present = upper::validate_header(image[512..536].try_into().unwrap())
        .map_err(|_| CpuMemoryError::GeneralProtection(0))?;
    if request & 6 != 0 {
        read_part(memory, address, 24, &mut image[24..28])?;
        xstate::validate_mxcsr(u32::from_le_bytes(image[24..28].try_into().unwrap()))
            .map_err(|_| CpuMemoryError::GeneralProtection(0))?;
    }
    if request & present & 1 != 0 {
        read_part(memory, address, 0, &mut image[..24])?;
        read_part(memory, address, 32, &mut image[32..160])?;
    }
    if request & present & 2 != 0 {
        read_part(memory, address, 160, &mut image[160..288])?;
    }
    if request & present & 4 != 0 {
        read_part(memory, address, 576, &mut image[576..704])?;
    }
    upper::restore(request, x87, simd, upper_state, &image)
        .map_err(|_| CpuMemoryError::GeneralProtection(0))
}

#[cfg(test)]
#[path = "upper/tests.rs"]
mod tests;
