use crate::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, X87_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{SIMD_SIZE, X87_SIZE, decode_simd, decode_x87, encode_simd, encode_x87},
    },
    cpu::x86::{
        CpuMemoryPort32, CpuReadPort32, X87State,
        simd::SimdState,
        xstate::{self, IMAGE_SIZE},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

mod upper;

impl EngineInstance {
    pub fn cpu_xsave(
        &mut self,
        address: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.xsave_bound(None, address, low, high)
    }

    pub fn cpu_xsave_resident(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.xsave_bound(Some(id), address, low, high)
    }

    pub fn cpu_xrstor(&mut self, address: u32, low: u32, high: u32) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.xrstor_bound(address, low, high)
    }

    pub fn cpu_xrstor_resident(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        low: u32,
        high: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.xrstor_bound(address, low, high)
    }

    fn xstate_current(&self) -> Result<(X87State, SimdState), HostError> {
        let x87 = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        let simd = decode_simd(&self.arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        Ok((x87, simd))
    }

    fn xsave_bound(
        &mut self,
        resident: Option<u64>,
        address: u32,
        low: u32,
        high: u32,
    ) -> Result<StoreCompletion, HostError> {
        let (x87, simd) = self.xstate_current()?;
        if address & 63 != 0 {
            self.xstate_packet(Err(CpuMemoryError::GeneralProtection(0)))?;
            return Ok(StoreCompletion::Complete);
        }
        let request = xstate::requested(low, high);
        let version = self.memory.as_ref().unwrap().code_validation_version();
        let result = save(&mut self.cpu_memory_port32(), address, request, &x87, &simd);
        let changed =
            result.is_ok() && version != self.memory.as_ref().unwrap().code_validation_version();
        self.xstate_packet(result)?;
        self.cpu_store_currentness(changed, resident)
    }

    fn xrstor_bound(&mut self, address: u32, low: u32, high: u32) -> Result<(), HostError> {
        let (current_x87, current_simd) = self.xstate_current()?;
        if address & 63 != 0 {
            return self.xstate_packet(Err(CpuMemoryError::GeneralProtection(0)));
        }
        let request = xstate::requested(low, high);
        let restored = restore(
            &mut self.cpu_memory_port32(),
            address,
            request,
            &current_x87,
            &current_simd,
        );
        match restored {
            Ok((x87, simd)) => {
                let mut x87_record = [0; X87_SIZE];
                let mut simd_record = [0; SIMD_SIZE];
                encode_x87(&x87, &mut x87_record).map_err(|_| HostError::Infrastructure)?;
                encode_simd(&simd, &mut simd_record).map_err(|_| HostError::Infrastructure)?;
                if request & 1 != 0 {
                    self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&x87_record);
                }
                if request & 2 != 0 {
                    self.arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&simd_record);
                }
                self.xstate_packet(Ok(()))
            }
            Err(error) => self.xstate_packet(Err(error)),
        }
    }

    fn xstate_packet(&mut self, result: Result<(), CpuMemoryError>) -> Result<(), HostError> {
        encode_cpu_result(
            IMAGE_SIZE as u32,
            0,
            result,
            &mut self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}

fn part_address(base: u32, offset: u32) -> GuestAddress {
    GuestAddress(base.wrapping_add(offset))
}

fn read_part(
    memory: &mut impl CpuReadPort32,
    base: u32,
    offset: u32,
    bytes: &mut [u8],
) -> Result<(), CpuMemoryError> {
    memory
        .read(part_address(base, offset), bytes)
        .map_err(CpuMemoryError::from)
}

fn save(
    memory: &mut impl CpuMemoryPort32,
    address: u32,
    request: u64,
    x87: &X87State,
    simd: &SimdState,
) -> Result<(), CpuMemoryError> {
    let mut image = [0; IMAGE_SIZE];
    // even an empty request performs the header read/modify/write.
    memory
        .read_modify(part_address(address, 512), &mut image[512..520])
        .map_err(CpuMemoryError::from)?;
    xstate::save(request, x87, simd, &mut image);
    let mut spans = [(GuestAddress(0), &[][..]); 5];
    let mut count = 0;
    for (selected, start, end) in [
        (request & 1 != 0, 0, 24),
        (request & 2 != 0, 24, 32),
        (request & 1 != 0, 32, 160),
        (request & 2 != 0, 160, 288),
        (true, 512, 520),
    ] {
        if selected {
            spans[count] = (part_address(address, start as u32), &image[start..end]);
            count += 1;
        }
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
) -> Result<(X87State, SimdState), CpuMemoryError> {
    let mut image = [0; IMAGE_SIZE];
    read_part(memory, address, 512, &mut image[512..536])?;
    let present = xstate::validate_header(image[512..536].try_into().unwrap())
        .map_err(|_| CpuMemoryError::GeneralProtection(0))?;
    if request & 2 != 0 {
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
    xstate::restore(request, x87, simd, &image).map_err(|_| CpuMemoryError::GeneralProtection(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::arena::TRANSFER_OFFSET,
        cpu::x86::{Register32, State32},
        process::{
            CallError,
            calls::{
                call::{PendingCall, PendingOwner},
                callback::{SuspendedCallback, SuspendedRecord},
                context::ExtendedCpuState,
            },
        },
        windows::{CallFrame32, CallingConvention32},
    };

    fn engine() -> (EngineInstance, u64) {
        let mut engine = EngineInstance::new(2, 7).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x2000, 1, 3).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0xeb, 0xfe])
            .unwrap();
        let descriptor = [0, 0x10, 0, 0, 2, 0, 0, 0];
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&descriptor);
        engine.compile(1).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&descriptor);
        let id = engine.compile_resident(1).unwrap().get();
        (engine, id)
    }

    fn blocked(engine: &mut EngineInstance, id: u64, error: HostError) {
        let before = engine.arena().to_vec();
        assert_eq!(engine.cpu_xsave(0x6001, 3, 0), Err(error));
        assert_eq!(engine.cpu_xrstor(0x6001, 3, 0), Err(error));
        assert_eq!(
            engine.cpu_xsave_resident(engine.key, id, 0x6001, 3, 0),
            Err(error)
        );
        assert_eq!(
            engine.cpu_xrstor_resident(engine.key, id, 0x6001, 3, 0),
            Err(error)
        );
        assert_eq!(engine.arena(), before);
    }

    #[test]
    fn xstate_pending_callback_wrong_owner_stale_and_closed_precede_all_effects() {
        let (mut engine, id) = engine();
        let mut state = State32::default();
        state.registers[Register32::Esp.index()] = 0x2000;
        let frame = CallFrame32::capture(
            engine.memory().unwrap(),
            state,
            CallingConvention32::Cdecl,
            0,
        )
        .unwrap();
        engine.pending_call = Some(PendingCall {
            token: 1,
            owner: PendingOwner::Resident(id),
            frame,
            state: engine.arena()[..56].try_into().unwrap(),
            exit: engine.arena()[56..96].try_into().unwrap(),
            extended: ExtendedCpuState::capture(engine.arena()).unwrap(),
        });
        blocked(&mut engine, id, HostError::Call(CallError::Busy));
        let outer = engine.pending_call.take().unwrap();
        engine.callback = Some(SuspendedCallback {
            outer,
            record: SuspendedRecord::Resident {
                record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                    token: 2,
                    outer_token: 1,
                    phase: 1,
                    outcome: 0,
                    entry_pc: 0x1000,
                    entry_esp: 0x2000,
                    return_pc: 0x1000,
                    return_id: 7,
                    stack_words: 0,
                    result: 0,
                    outer_unit_id: id,
                    callback_unit_id: id,
                },
                authorized: false,
                active_unit_id: id,
            },
        });
        blocked(&mut engine, id, HostError::Call(CallError::Busy));
        engine.callback = None;
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.cpu_xsave_resident(engine.key + 1, id, 0x2000, 3, 0),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(
            engine.cpu_xrstor_resident(engine.key + 1, id, 0x2000, 3, 0),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(engine.arena(), before);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0xeb, 0xfe])
            .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.cpu_xsave(0x2000, 3, 0),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.cpu_xrstor(0x2000, 3, 0),
            Err(HostError::CodeInvalidated)
        );
        let error = HostError::Resident(crate::cpu::dbt::RegistryError::CodeInvalidated);
        assert_eq!(
            engine.cpu_xsave_resident(engine.key, id, 0x2000, 3, 0),
            Err(error)
        );
        assert_eq!(
            engine.cpu_xrstor_resident(engine.key, id, 0x2000, 3, 0),
            Err(error)
        );
        assert_eq!(engine.arena(), before);
        engine.close();
        blocked(&mut engine, id, HostError::Closed);
    }
}

#[cfg(test)]
#[path = "xstate/paging.rs"]
mod paging_tests;
