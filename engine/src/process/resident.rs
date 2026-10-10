#![forbid(unsafe_code)]

use super::{CallError, EngineInstance, HostError};
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{
        BlockSpec, CompileLimits, CompiledRegion, GateSpec, RegistryError, RegistryLimits,
        ResidentRegistry, UnitId,
    },
    memory::GuestAddress,
};

impl EngineInstance {
    /// call only after generated execution returns; discarded module pointers expire.
    /// no engine installation record does not certify foreign host references or quiescence.
    pub fn discard_unacknowledged_resident(&mut self, key: u64, id: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .get_raw(memory, id)
            .map_err(HostError::Resident)?;
        if self
            .resident_installations
            .iter()
            .flatten()
            .any(|installed| installed.unit_id == id)
        {
            return Err(HostError::InvalidRequest);
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        self.resident
            .as_mut()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .discard_current_raw(memory, id)
            .map_err(HostError::Resident)
    }

    /// call only after generated execution returns and the host clears the victim slot.
    /// discarded module pointers expire; cached host references do not certify quiescence.
    pub fn discard_installed_resident(
        &mut self,
        key: u64,
        id: u64,
        expected_slot: u32,
    ) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .get_raw(memory, id)
            .map_err(HostError::Resident)?;
        let slot = expected_slot as usize;
        if !self
            .resident_installations
            .get(slot)
            .is_some_and(|installed| installed.is_some_and(|entry| entry.unit_id == id))
        {
            return Err(HostError::InvalidRequest);
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        self.resident
            .as_mut()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .discard_current_raw(memory, id)
            .map_err(HostError::Resident)?;
        self.resident_installations[slot] = None;
        Ok(())
    }

    /// call only after generated execution returns; retired module pointers expire.
    pub fn retire_stale_resident(&mut self, key: u64, id: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .check_stale_raw(memory, id)
            .map_err(HostError::Resident)?;
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        self.resident
            .as_mut()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .retire_stale_raw(memory, id)
            .map_err(HostError::Resident)?;
        for installed in &mut self.resident_installations {
            if installed.is_some_and(|entry| entry.unit_id == id) {
                *installed = None;
            }
        }
        Ok(())
    }

    pub fn compile_resident(&mut self, count: u32) -> Result<UnitId, HostError> {
        self.compile_resident_with_gates(count, 0)
    }

    pub fn compile_resident_with_gates(
        &mut self,
        count: u32,
        gate_count: u32,
    ) -> Result<UnitId, HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.compile_resident_descriptors(count, gate_count, None)
    }

    pub fn compile_resident_entries(
        &mut self,
        count: u32,
        gate_count: u32,
    ) -> Result<UnitId, HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Self::check_region_counts(count, gate_count)?;
        let mut entries = [GuestAddress(0); 8];
        let mut gates = [GateSpec {
            entry: GuestAddress(0),
            id: 0,
        }; 8];
        let transfer = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
        for (index, entry) in entries[..count as usize].iter_mut().enumerate() {
            let offset = index * 4;
            *entry = GuestAddress(u32::from_le_bytes(
                transfer[offset..offset + 4].try_into().unwrap(),
            ));
        }
        for (index, gate) in gates[..gate_count as usize].iter_mut().enumerate() {
            let offset = count as usize * 4 + index * 8;
            *gate = GateSpec {
                entry: GuestAddress(u32::from_le_bytes(
                    transfer[offset..offset + 4].try_into().unwrap(),
                )),
                id: u32::from_le_bytes(transfer[offset + 4..offset + 8].try_into().unwrap()),
            };
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let entries = &entries[..count as usize];
        let gates = &gates[..gate_count as usize];
        if let Some(registry) = self.resident.as_mut() {
            registry
                .compile_entries_bound(memory, entries, gates, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)
        } else {
            let mut registry = ResidentRegistry::new(memory, RegistryLimits::default())
                .map_err(HostError::Resident)?;
            let id = registry
                .compile_entries_bound(memory, entries, gates, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)?;
            self.resident = Some(registry);
            Ok(id)
        }
    }

    pub(super) fn compile_resident_descriptors(
        &mut self,
        count: u32,
        gate_count: u32,
        required_entry: Option<u32>,
    ) -> Result<UnitId, HostError> {
        Self::check_region_counts(count, gate_count)?;
        let mut specs = [BlockSpec {
            entry: GuestAddress(0),
            byte_length: 0,
        }; 8];
        let transfer = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
        for (index, spec) in specs[..count as usize].iter_mut().enumerate() {
            let offset = index * 8;
            *spec = BlockSpec {
                entry: GuestAddress(u32::from_le_bytes(
                    transfer[offset..offset + 4].try_into().unwrap(),
                )),
                byte_length: u32::from_le_bytes(
                    transfer[offset + 4..offset + 8].try_into().unwrap(),
                ),
            };
        }
        let mut gates = [GateSpec {
            entry: GuestAddress(0),
            id: 0,
        }; 8];
        for (index, gate) in gates[..gate_count as usize].iter_mut().enumerate() {
            let offset = count as usize * 8 + index * 8;
            *gate = GateSpec {
                entry: GuestAddress(u32::from_le_bytes(
                    transfer[offset..offset + 4].try_into().unwrap(),
                )),
                id: u32::from_le_bytes(transfer[offset + 4..offset + 8].try_into().unwrap()),
            };
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let specs = &specs[..count as usize];
        let gates = &gates[..gate_count as usize];
        if required_entry
            .is_some_and(|pc| specs[0].entry.0 != pc || gates.iter().any(|gate| gate.entry.0 == pc))
        {
            return Err(HostError::InvalidRequest);
        }
        if let Some(registry) = self.resident.as_mut() {
            registry
                .compile_bound(memory, specs, gates, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)
        } else {
            let mut registry = ResidentRegistry::new(memory, RegistryLimits::default())
                .map_err(HostError::Resident)?;
            let id = registry
                .compile_bound(memory, specs, gates, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)?;
            self.resident = Some(registry);
            Ok(id)
        }
    }

    pub fn lookup_resident(&self, pc: u32) -> Result<UnitId, HostError> {
        let memory = self.memory()?;
        let pc = GuestAddress(pc);
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::NotFound { pc }))?
            .lookup(memory, pc)
            .map_err(HostError::Resident)
    }

    pub fn resident_bytes(&self, id: u64) -> Result<&[u8], HostError> {
        let memory = self.memory()?;
        let registry = self
            .resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?;
        registry
            .get_raw(memory, id)
            .map_err(HostError::Resident)?
            .wasm_bytes(memory)
            .map_err(|_| HostError::Resident(RegistryError::CodeInvalidated))
    }

    pub fn guard_resident(&self, key: u64, id: u64) -> Result<(), HostError> {
        self.guard_resident_unit(key, id)?;
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if let Some(callback) = self.callback.as_ref() {
            let Some(record) = callback.authorized_resident_record() else {
                return Err(HostError::Call(CallError::Busy));
            };
            if Some(id) != callback.authorized_resident_active_id() {
                return Err(HostError::Call(CallError::Busy));
            }
            self.guard_resident_unit(key, record.callback_unit_id)?;
            self.guard_resident_unit(key, record.outer_unit_id)?;
        }
        Ok(())
    }

    pub(super) fn guard_resident_unit(
        &self,
        key: u64,
        id: u64,
    ) -> Result<&CompiledRegion, HostError> {
        let memory = self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .get_raw(memory, id)
            .map_err(HostError::Resident)
    }
}

#[cfg(test)]
mod resident_disposal_tests {
    use super::*;
    use crate::{
        abi::x86::{encode_exit_v3, encode_state},
        cpu::{ExecutionExit, ExitReason, dbt::RegistryUsage, x86::State32},
        process::ResidentInstallation,
        windows::{
            CallFrame32, CallingConvention32, ProcessContext32, WindowsApi32, WindowsOutcome32,
        },
    };

    const KEY: u64 = 0xa379_1020_3040_5060;
    const PAGES: [u32; 3] = [0x1000, 0x2000, 0x8000];

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Retained {
        arena: Vec<u8>,
        address: usize,
        generation: u32,
        call_token: u32,
        exit_code: Option<u32>,
        calls: (bool, bool),
        image: (Option<crate::loader::ImageMetadata32>, bool, bool),
        last_error: u32,
        memory: (u64, u32, u32),
        ram: Vec<Vec<u8>>,
        replacement: (Vec<u8>, usize),
        dispatcher: (Vec<u8>, usize),
        usage: RegistryUsage,
        units: Vec<(u64, Vec<u8>, usize)>,
        installations: Vec<Option<ResidentInstallation>>,
    }

    fn last_error(engine: &EngineInstance, frame: &CallFrame32) -> u32 {
        let (outcome, _) = engine
            .windows_thread
            .prepare(
                WindowsApi32::GetLastError,
                frame,
                ProcessContext32::default(),
            )
            .unwrap();
        let WindowsOutcome32::Return(error) = outcome else {
            unreachable!()
        };
        error
    }

    fn retained(engine: &EngineInstance, ids: &[u64], frame: &CallFrame32) -> Retained {
        // terminal ownership is observed through retained private owners, not public getters.
        let memory = engine.memory.as_ref().unwrap();
        let owned = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
        Retained {
            arena: engine.arena().to_vec(),
            address: engine.arena_address(),
            generation: engine.generation,
            call_token: engine.call_token,
            exit_code: engine.exit_code,
            calls: (engine.pending_call.is_some(), engine.callback.is_some()),
            image: (
                engine.image,
                engine.image_input.is_some(),
                engine.image_started,
            ),
            last_error: last_error(engine, frame),
            memory: (
                memory.identity(),
                memory.capacity_pages(),
                memory.mapped_pages(),
            ),
            ram: PAGES
                .into_iter()
                .map(|pc| {
                    let mut bytes = vec![0; 4096];
                    memory.read(GuestAddress(pc), &mut bytes).unwrap();
                    bytes
                })
                .collect(),
            replacement: owned(
                engine
                    .artifact
                    .as_ref()
                    .unwrap()
                    .wasm_bytes(memory)
                    .unwrap(),
            ),
            dispatcher: owned(engine.dispatcher.as_ref().unwrap()),
            usage: engine.resident.as_ref().unwrap().usage(),
            units: ids
                .iter()
                .map(|&id| {
                    let bytes = engine
                        .resident
                        .as_ref()
                        .unwrap()
                        .get_raw(memory, id)
                        .unwrap()
                        .wasm_bytes(memory)
                        .unwrap();
                    (id, bytes.to_vec(), bytes.as_ptr() as usize)
                })
                .collect(),
            installations: engine.resident_installations.to_vec(),
        }
    }

    fn fixture() -> (EngineInstance, [u64; 2], CallFrame32) {
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        for page in PAGES {
            engine.map(page, 1, 7).unwrap();
        }
        for (pc, bytes) in [
            (0x1000, &[0x90, 0xeb, 0][..]),
            (0x2000, &[0x90, 0xeb, 0][..]),
            (0x2100, &[0x0f, 0x0b][..]),
        ] {
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(pc, bytes.len() as u32).unwrap();
        }
        let mut ids = [0; 2];
        for (index, words) in [
            vec![0x1000_u32, 3],
            vec![0x2000, 3, 0x2100, 2, 0x2100, WindowsApi32::ExitProcess.id()],
        ]
        .into_iter()
        .enumerate()
        {
            for (slot, word) in words.into_iter().enumerate() {
                let at = 140 + slot * 4;
                engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
            }
            ids[index] = engine
                .compile_resident_with_gates(if index == 0 { 1 } else { 2 }, index as u32)
                .unwrap()
                .get();
        }
        engine
            .acknowledge_resident_installation(KEY, ids[1], 7)
            .unwrap();
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
        engine.arena_mut().unwrap()[144..148].copy_from_slice(&3_u32.to_le_bytes());
        engine.compile(1).unwrap();
        engine.write32(0x8f00, 0x2001).unwrap();
        engine.write32(0x8f04, 0).unwrap();
        let state = State32 {
            registers: [10, 2, 3, 4, 0x8f00, 6, 7, 8],
            eip: 0x2100,
            eflags: 0xcd7,
        };
        let frame = CallFrame32::capture(
            engine.memory().unwrap(),
            state,
            CallingConvention32::Stdcall,
            0,
        )
        .unwrap();
        engine.windows_thread = engine.windows_thread.allocation_failed();
        assert_eq!(last_error(&engine, &frame), 8);
        (engine, ids, frame)
    }

    #[test]
    fn discard_lifecycle_refusals_preserve_retained_owners_until_close() {
        for code in [None, Some(0), Some(u32::MAX)] {
            let (mut engine, ids, frame) = fixture();
            if let Some(code) = code {
                engine.write32(0x8f04, code).unwrap();
                encode_state(frame.state(), &mut engine.arena_mut().unwrap()[..56]).unwrap();
                // this typed stop proves terminal ownership; actual execution is separate.
                encode_exit_v3(
                    &ExecutionExit {
                        retired: 1,
                        reason: ExitReason::Gate {
                            id: WindowsApi32::ExitProcess.id(),
                        },
                    },
                    &mut engine.arena_mut().unwrap()[56..96],
                )
                .unwrap();
                let token = engine
                    .capture_resident_call(KEY, ids[1], CallingConvention32::Stdcall, 1)
                    .unwrap()
                    .token;
                engine
                    .complete_resident_windows_call(KEY, ids[1], token)
                    .unwrap();
                assert_eq!(engine.exit_code, Some(code));
                // lifecycle authority precedes a malformed public frame and cancellation.
                let arena = engine.arena.as_mut().get_mut();
                arena[..96].fill(0xff);
                arena[96..100].copy_from_slice(&1_u32.to_le_bytes());
            }
            let before = retained(&engine, &ids, &frame);
            let memory = engine.memory.as_ref().unwrap();
            let versions = PAGES.map(|pc| memory.snapshot_code(GuestAddress(pc), 4096).unwrap());
            if code.is_some() {
                for key in [0, KEY, KEY ^ (1_u64 << 32)] {
                    for id in [0, ids[0], ids[1], u64::MAX] {
                        assert_eq!(
                            engine.discard_unacknowledged_resident(key, id),
                            Err(HostError::ProcessExited)
                        );
                        assert_eq!(retained(&engine, &ids, &frame), before);
                    }
                }
            } else {
                assert_eq!(
                    engine.discard_unacknowledged_resident(KEY, ids[1]),
                    Err(HostError::InvalidRequest)
                );
                assert_eq!(retained(&engine, &ids, &frame), before);
                engine.discard_unacknowledged_resident(KEY, ids[0]).unwrap();
                let mut expected = before.clone();
                expected.units.remove(0);
                expected.usage.units -= 1;
                expected.usage.wasm_bytes -= before.units[0].1.len();
                assert_eq!(retained(&engine, &ids[1..], &frame), expected);
            }
            let memory = engine.memory.as_ref().unwrap();
            for version in &versions {
                assert!(memory.is_code_current(version));
            }
            let arena = engine.arena().to_vec();
            let address = engine.arena_address();
            let generation = engine.generation;
            let token = engine.call_token;
            engine.close();
            engine.close();
            assert_eq!(engine.arena(), arena);
            assert_eq!(engine.arena_address(), address);
            assert_eq!(engine.key, KEY);
            assert_eq!(engine.generation, generation);
            assert_eq!(engine.generation(), 0);
            assert_eq!(engine.call_token, token);
            assert_eq!(engine.exit_code, code);
            assert_eq!(last_error(&engine, &frame), 8);
            assert!(
                engine.memory.is_none()
                    && engine.artifact.is_none()
                    && engine.dispatcher.is_none()
                    && engine.resident.is_none()
            );
            assert!(
                engine.pending_call.is_none()
                    && engine.callback.is_none()
                    && engine.image_input.is_none()
            );
            assert!(engine.resident_installations.iter().all(Option::is_none));
            for (key, id) in [(KEY, ids[1]), (0, 0), (KEY ^ (1_u64 << 32), u64::MAX)] {
                assert_eq!(
                    engine.discard_unacknowledged_resident(key, id),
                    Err(HostError::Closed)
                );
                assert_eq!(engine.arena(), arena);
            }
        }
    }
}
