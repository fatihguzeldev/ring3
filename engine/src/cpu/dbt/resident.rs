use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    cpu::x86::decode::CodeFetchContext,
    memory::{AddressSpace, GuestAddress},
};

use super::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, CompiledRegion, GateSpec,
    PreparedRegion, artifact::emit_prepared, cold::prepare_resident_entry_region, compile_region,
    region::prepare_resident_region, wasm::EmbeddedBinding,
};

const MAX_UNITS: usize = 8;
const MAX_BYTES: usize = MAX_UNITS * 65_536;
const CONFIGURED_MAX_UNITS: usize = 4096;
const CONFIGURED_MAX_BYTES: usize = 256 * 1024 * 1024;
static NEXT_UNIT_ID: AtomicU64 = AtomicU64::new(1);

enum EmissionPolicy {
    WholeUnit(CompileLimits),
    EntryPrefix(CompileLimits),
}

/// an immutable id in one engine allocation domain, not a persisted handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnitId(u64);

impl UnitId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryLimits {
    pub units: usize,
    pub wasm_bytes: usize,
}

impl Default for RegistryLimits {
    fn default() -> Self {
        Self {
            units: MAX_UNITS,
            wasm_bytes: MAX_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryUsage {
    pub units: usize,
    pub wasm_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidLimits,
    Allocation,
    WrongAddressSpace,
    UnitCapacity,
    ByteCapacity,
    Compile(CompileError),
    InstructionOverlap { pc: GuestAddress },
    IdentityExhausted,
    InvalidUnit,
    CurrentUnit,
    CodeInvalidated,
    NotFound { pc: GuestAddress },
}

#[derive(Debug)]
struct ResidentUnit {
    id: UnitId,
    region: CompiledRegion,
}

#[derive(Debug)]
pub struct ResidentRegistry {
    memory_identity: u64,
    limits: RegistryLimits,
    entries: Vec<ResidentUnit>,
    wasm_bytes: usize,
}

impl ResidentRegistry {
    pub fn new(memory: &AddressSpace, limits: RegistryLimits) -> Result<Self, RegistryError> {
        if !(1..=CONFIGURED_MAX_UNITS).contains(&limits.units)
            || !(1..=CONFIGURED_MAX_BYTES).contains(&limits.wasm_bytes)
        {
            return Err(RegistryError::InvalidLimits);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(limits.units)
            .map_err(|_| RegistryError::Allocation)?;
        Ok(Self {
            memory_identity: memory.identity(),
            limits,
            entries,
            wasm_bytes: 0,
        })
    }

    pub fn compile(
        &mut self,
        memory: &AddressSpace,
        specs: &[BlockSpec],
        limits: CompileLimits,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let region = compile_region(memory, specs, limits).map_err(RegistryError::Compile)?;
        let wasm_bytes = self.admit(memory, &region, None)?;
        let id = allocate_id(&NEXT_UNIT_ID)?;
        self.publish(id, region, wasm_bytes);
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_bound(
        &mut self,
        memory: &AddressSpace,
        specs: &[BlockSpec],
        gates: &[GateSpec],
        limits: CompileLimits,
        key: u64,
        cs: Option<CodeFetchContext>,
        authority: crate::cpu::x86::decode::DecodeAuthority,
    ) -> Result<UnitId, RegistryError> {
        self.compile_bound_with_counter(
            memory,
            specs,
            gates,
            limits,
            key,
            cs,
            authority,
            &NEXT_UNIT_ID,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_entries_bound(
        &mut self,
        memory: &AddressSpace,
        entries: &[GuestAddress],
        gates: &[GateSpec],
        limits: CompileLimits,
        key: u64,
        cs: Option<CodeFetchContext>,
        authority: crate::cpu::x86::decode::DecodeAuthority,
    ) -> Result<UnitId, RegistryError> {
        self.compile_entries_bound_with_counter(
            memory,
            entries,
            gates,
            limits,
            key,
            cs,
            authority,
            &NEXT_UNIT_ID,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_bound_with_counter(
        &mut self,
        memory: &AddressSpace,
        specs: &[BlockSpec],
        gates: &[GateSpec],
        limits: CompileLimits,
        key: u64,
        cs: Option<CodeFetchContext>,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        counter: &AtomicU64,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let prepared = prepare_resident_region(memory, specs, limits, gates, cs, authority)
            .map_err(RegistryError::Compile)?;
        self.compile_prepared_bound(
            memory,
            prepared,
            EmissionPolicy::WholeUnit(limits),
            key,
            cs,
            counter,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_entries_bound_with_counter(
        &mut self,
        memory: &AddressSpace,
        entries: &[GuestAddress],
        gates: &[GateSpec],
        limits: CompileLimits,
        key: u64,
        cs: Option<CodeFetchContext>,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        counter: &AtomicU64,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let prepared = prepare_resident_entry_region(memory, entries, limits, gates, cs, authority)
            .map_err(RegistryError::Compile)?;
        self.compile_prepared_bound(
            memory,
            prepared,
            EmissionPolicy::EntryPrefix(limits),
            key,
            cs,
            counter,
        )
    }

    pub(crate) fn compile_runtime_bound(
        &mut self,
        memory: &AddressSpace,
        entry: GuestAddress,
        limits: CompileLimits,
        key: u64,
        cs: CodeFetchContext,
        authority: crate::cpu::x86::decode::DecodeAuthority,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let prepared =
            super::cold::prepare_runtime_entry(memory, entry, limits, Some(cs), authority)
                .map_err(RegistryError::Compile)?;
        self.compile_prepared_bound(
            memory,
            prepared,
            EmissionPolicy::EntryPrefix(limits),
            key,
            Some(cs),
            &NEXT_UNIT_ID,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_cached_bound(
        &mut self,
        memory: &AddressSpace,
        entry: GuestAddress,
        gate: Option<GateSpec>,
        limits: CompileLimits,
        key: u64,
        cs: CodeFetchContext,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        tier: super::CompilationTier32,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let prepared = if let Some(gate) = gate {
            prepare_resident_entry_region(memory, &[entry], limits, &[gate], Some(cs), authority)
        } else {
            super::cold::prepare_runtime_entry_until(
                memory,
                entry,
                limits,
                Some(cs),
                authority,
                &|pc| self.lookup_bound(memory, pc, cs).is_ok(),
            )
        }
        .map_err(RegistryError::Compile)?;
        let id = allocate_id(&NEXT_UNIT_ID)?;
        let region = super::artifact::emit_entry_prepared_tier(
            memory,
            prepared,
            limits,
            Some(EmbeddedBinding::Resident { key, id: id.get() }),
            Some(cs),
            tier,
        )
        .map_err(RegistryError::Compile)?;
        let bytes = self.admit(memory, &region, Some(cs))?;
        self.publish(id, region, bytes);
        Ok(id)
    }

    fn compile_prepared_bound(
        &mut self,
        memory: &AddressSpace,
        prepared: PreparedRegion,
        policy: EmissionPolicy,
        key: u64,
        cs: Option<CodeFetchContext>,
        counter: &AtomicU64,
    ) -> Result<UnitId, RegistryError> {
        // final emission can fail after reservation; unpublished ids are never reused.
        let id = allocate_id(counter)?;
        let binding = Some(EmbeddedBinding::Resident { key, id: id.get() });
        let region = match policy {
            EmissionPolicy::EntryPrefix(limits) => {
                super::artifact::emit_entry_prepared(memory, prepared, limits, binding, cs)
            }
            EmissionPolicy::WholeUnit(limits) => emit_prepared(prepared, limits, binding),
        }
        .map_err(RegistryError::Compile)?;
        let wasm_bytes = self.admit(memory, &region, cs)?;
        self.publish(id, region, wasm_bytes);
        Ok(id)
    }

    fn admit(
        &self,
        memory: &AddressSpace,
        region: &CompiledRegion,
        cs: Option<CodeFetchContext>,
    ) -> Result<usize, RegistryError> {
        let bytes = region
            .wasm_bytes(memory)
            .map_err(|ArtifactError::CodeInvalidated| RegistryError::CodeInvalidated)?
            .len();
        let wasm_bytes = self
            .wasm_bytes
            .checked_add(bytes)
            .filter(|&total| total <= self.limits.wasm_bytes)
            .ok_or(RegistryError::ByteCapacity)?;
        for pc in region.executable_addresses() {
            if self.entries.iter().any(|unit| {
                unit.region.matches_decode_key(region.decode_key())
                    && unit.region.contains_executable(pc.0)
                    && unit.region.wasm_bytes(memory).is_ok()
                    && (unit.region.contains_material(pc.0)
                        || !region.contains_material(pc.0)
                        || unit.region.context_fault(pc.0).is_none_or(|fault| {
                            cs.is_none_or(|cs| fault.remains_denied(memory, cs))
                        }))
            }) {
                return Err(RegistryError::InstructionOverlap { pc });
            }
        }
        Ok(wasm_bytes)
    }

    fn publish(&mut self, id: UnitId, region: CompiledRegion, wasm_bytes: usize) {
        // the reserved vector makes publication infallible after the id claim.
        self.entries.push(ResidentUnit { id, region });
        self.wasm_bytes = wasm_bytes;
    }

    pub(crate) fn get_raw(
        &self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<&CompiledRegion, RegistryError> {
        self.get(memory, UnitId(id))
    }

    /// identity-only view for a consumed execution lease; this is not an execution guard.
    pub(crate) fn get_retained_raw(
        &self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<&CompiledRegion, RegistryError> {
        self.check_memory(memory)?;
        self.entries
            .iter()
            .find(|unit| unit.id.get() == id)
            .map(|unit| &unit.region)
            .ok_or(RegistryError::InvalidUnit)
    }

    pub(crate) fn discard_quiescent_raw(
        &mut self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<(), RegistryError> {
        self.check_memory(memory)?;
        let index = self
            .entries
            .iter()
            .position(|unit| unit.id.get() == id)
            .ok_or(RegistryError::InvalidUnit)?;
        self.remove_unit(index)
    }

    pub fn get(&self, memory: &AddressSpace, id: UnitId) -> Result<&CompiledRegion, RegistryError> {
        self.check_memory(memory)?;
        let unit = self
            .entries
            .iter()
            .find(|unit| unit.id == id)
            .ok_or(RegistryError::InvalidUnit)?;
        unit.region
            .wasm_bytes(memory)
            .map_err(|ArtifactError::CodeInvalidated| RegistryError::CodeInvalidated)?;
        Ok(&unit.region)
    }

    pub fn lookup(&self, memory: &AddressSpace, pc: GuestAddress) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        let mut stale = false;
        for unit in &self.entries {
            if unit
                .region
                .matches_decode_key(crate::cpu::x86::decode::CodeDecodeKey::FLAT)
                && unit.region.contains_executable(pc.0)
            {
                match unit.region.wasm_bytes(memory) {
                    Ok(_) => return Ok(unit.id),
                    Err(ArtifactError::CodeInvalidated) => stale = true,
                }
            }
        }
        Err(if stale {
            RegistryError::CodeInvalidated
        } else {
            RegistryError::NotFound { pc }
        })
    }

    pub(crate) fn lookup_bound(
        &self,
        memory: &AddressSpace,
        pc: GuestAddress,
        cs: CodeFetchContext,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        let mut stale = false;
        for material in [true, false] {
            for unit in &self.entries {
                if !unit.region.matches_decode_key(cs.decode_key()) {
                    continue;
                }
                let matches = if material {
                    unit.region.contains_material(pc.0)
                } else {
                    unit.region.context_fault(pc.0).is_some()
                };
                if matches {
                    match unit.region.wasm_bytes(memory) {
                        Ok(_)
                            if material
                                || unit
                                    .region
                                    .context_fault(pc.0)
                                    .is_some_and(|fault| fault.remains_denied(memory, cs)) =>
                        {
                            return Ok(unit.id);
                        }
                        Ok(_) => {}
                        Err(ArtifactError::CodeInvalidated) => stale = true,
                    }
                }
            }
        }
        Err(if stale {
            RegistryError::CodeInvalidated
        } else {
            RegistryError::NotFound { pc }
        })
    }

    pub fn usage(&self) -> RegistryUsage {
        RegistryUsage {
            units: self.entries.len(),
            wasm_bytes: self.wasm_bytes,
        }
    }

    pub fn retire_stale(&mut self, memory: &AddressSpace, id: UnitId) -> Result<(), RegistryError> {
        let index = self.stale_index(memory, id.get())?;
        self.remove_unit(index)
    }

    pub(crate) fn discard_current_raw(
        &mut self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<(), RegistryError> {
        self.check_memory(memory)?;
        let index = self
            .entries
            .iter()
            .position(|unit| unit.id.get() == id)
            .ok_or(RegistryError::InvalidUnit)?;
        self.entries[index]
            .region
            .wasm_bytes(memory)
            .map_err(|ArtifactError::CodeInvalidated| RegistryError::CodeInvalidated)?;
        self.remove_unit(index)
    }

    fn remove_unit(&mut self, index: usize) -> Result<(), RegistryError> {
        let bytes = self.entries[index].region.retained_wasm_byte_len();
        let remaining = self
            .wasm_bytes
            .checked_sub(bytes)
            .ok_or(RegistryError::InvalidLimits)?;
        self.entries.remove(index);
        self.wasm_bytes = remaining;
        Ok(())
    }

    pub(crate) fn retire_stale_raw(
        &mut self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<(), RegistryError> {
        self.retire_stale(memory, UnitId(id))
    }

    pub(crate) fn check_stale_raw(
        &self,
        memory: &AddressSpace,
        id: u64,
    ) -> Result<(), RegistryError> {
        self.stale_index(memory, id).map(|_| ())
    }

    fn stale_index(&self, memory: &AddressSpace, id: u64) -> Result<usize, RegistryError> {
        self.check_memory(memory)?;
        let index = self
            .entries
            .iter()
            .position(|unit| unit.id.get() == id)
            .ok_or(RegistryError::InvalidUnit)?;
        match self.entries[index].region.wasm_bytes(memory) {
            Ok(_) => Err(RegistryError::CurrentUnit),
            Err(ArtifactError::CodeInvalidated) => Ok(index),
        }
    }

    fn check_memory(&self, memory: &AddressSpace) -> Result<(), RegistryError> {
        if memory.identity() != self.memory_identity {
            return Err(RegistryError::WrongAddressSpace);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "resident/tests/entries.rs"]
mod resident_entries_tests;

#[cfg(test)]
#[path = "resident/tests/retirement.rs"]
mod resident_retirement_tests;

fn allocate_id(counter: &AtomicU64) -> Result<UnitId, RegistryError> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |identity| {
            if identity == 0 {
                None
            } else {
                identity.checked_add(1)
            }
        })
        .map(UnitId)
        .map_err(|_| RegistryError::IdentityExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn guarded_admission_never_reuses_unpublished_ids_or_changes_retained_units() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 2).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        for pc in [0x1000, 0x2000] {
            memory.write(GuestAddress(pc), &[0x40, 0xeb, 0]).unwrap();
        }
        let specs = |pc| {
            [BlockSpec {
                entry: GuestAddress(pc),
                byte_length: 3,
            }]
        };
        let counter = AtomicU64::new(1);
        let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
        let first = registry
            .compile_bound_with_counter(
                &memory,
                &specs(0x1000),
                &[],
                CompileLimits::default(),
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter,
            )
            .unwrap();
        assert_eq!(first.get(), 1);
        let bytes = registry
            .get(&memory, first)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap()
            .to_vec();
        let pointer = registry
            .get(&memory, first)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap()
            .as_ptr();
        let usage = registry.usage();
        assert_eq!(
            registry.compile_bound_with_counter(
                &memory,
                &specs(0x2000),
                &[],
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter
            ),
            Err(RegistryError::Compile(CompileError::WasmLimit))
        );
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(
            registry.compile_bound_with_counter(
                &memory,
                &specs(0x1000),
                &[],
                CompileLimits::default(),
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter
            ),
            Err(RegistryError::InstructionOverlap {
                pc: GuestAddress(0x1000)
            })
        );
        assert_eq!(counter.load(Ordering::Relaxed), 4);
        for id in [2, 3] {
            assert_eq!(
                registry.get_raw(&memory, id).unwrap_err(),
                RegistryError::InvalidUnit
            );
        }
        assert_eq!(registry.usage(), usage);
        assert_eq!(
            registry
                .get(&memory, first)
                .unwrap()
                .wasm_bytes(&memory)
                .unwrap(),
            bytes
        );
        assert_eq!(
            registry
                .get(&memory, first)
                .unwrap()
                .wasm_bytes(&memory)
                .unwrap()
                .as_ptr(),
            pointer
        );
        let next = registry
            .compile_bound_with_counter(
                &memory,
                &specs(0x2000),
                &[],
                CompileLimits::default(),
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter,
            )
            .unwrap();
        assert_eq!(next.get(), 4);

        let counter = AtomicU64::new(1);
        let mut tight = ResidentRegistry::new(
            &memory,
            RegistryLimits {
                units: 8,
                wasm_bytes: bytes.len(),
            },
        )
        .unwrap();
        let first = tight
            .compile_bound_with_counter(
                &memory,
                &specs(0x1000),
                &[],
                CompileLimits::default(),
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter,
            )
            .unwrap();
        let usage = tight.usage();
        assert_eq!(
            tight.compile_bound_with_counter(
                &memory,
                &specs(0x2000),
                &[],
                CompileLimits::default(),
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter
            ),
            Err(RegistryError::ByteCapacity)
        );
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(tight.usage(), usage);
        assert_eq!(
            tight.get_raw(&memory, 2).unwrap_err(),
            RegistryError::InvalidUnit
        );
        assert_eq!(
            tight
                .get(&memory, first)
                .unwrap()
                .wasm_bytes(&memory)
                .unwrap(),
            bytes
        );
        assert_eq!(
            tight.compile_bound_with_counter(
                &memory,
                &specs(0x2000),
                &[],
                CompileLimits {
                    blocks: 0,
                    ..CompileLimits::default()
                },
                7,
                None,
                crate::cpu::x86::decode::DecodeAuthority::default(),
                &counter
            ),
            Err(RegistryError::Compile(CompileError::InvalidLimits))
        );
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn unit_ids_are_nonzero_monotonic_and_never_wrap() {
        let counter = AtomicU64::new(1);
        let first = allocate_id(&counter).unwrap();
        let second = allocate_id(&counter).unwrap();
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_ne!(first, second);
        let boundary = AtomicU64::new(u64::MAX - 1);
        assert_eq!(allocate_id(&boundary).unwrap().get(), u64::MAX - 1);
        for _ in 0..2 {
            assert_eq!(
                allocate_id(&boundary),
                Err(RegistryError::IdentityExhausted)
            );
            assert_eq!(boundary.load(Ordering::Relaxed), u64::MAX);
        }
        let zero = AtomicU64::new(0);
        assert_eq!(allocate_id(&zero), Err(RegistryError::IdentityExhausted));
        assert_eq!(zero.load(Ordering::Relaxed), 0);
    }
}

#[cfg(test)]
mod decode_key_tests {
    use super::*;
    use crate::{
        cpu::x86::SegmentContext,
        memory::{PageRange, Permissions},
    };

    fn context(base: u32) -> CodeFetchContext {
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = base;
        CodeFetchContext::from_segments(&segments).unwrap()
    }

    #[test]
    fn same_logical_entry_can_retain_distinct_keys_without_wrong_key_stale_routing() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [0x1000, 0x5000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory.write(GuestAddress(page), &[0xeb, 0xfe]).unwrap();
        }
        let pc = GuestAddress(0x1000);
        let specs = [BlockSpec {
            entry: pc,
            byte_length: 2,
        }];
        let mut registry = ResidentRegistry::new(
            &memory,
            RegistryLimits {
                units: 2,
                wasm_bytes: MAX_BYTES,
            },
        )
        .unwrap();
        let flat = registry
            .compile_bound(
                &memory,
                &specs,
                &[],
                CompileLimits::default(),
                7,
                Some(context(0)),
                crate::cpu::x86::decode::DecodeAuthority::default(),
            )
            .unwrap();
        let based = registry
            .compile_bound(
                &memory,
                &specs,
                &[],
                CompileLimits::default(),
                7,
                Some(context(0x4000)),
                crate::cpu::x86::decode::DecodeAuthority::default(),
            )
            .unwrap();
        assert_ne!(flat, based);
        assert_eq!(registry.lookup_bound(&memory, pc, context(0)), Ok(flat));
        assert_eq!(
            registry.lookup_bound(&memory, pc, context(0x4000)),
            Ok(based)
        );
        memory.write(pc, &[0x90]).unwrap();
        assert_eq!(
            registry.lookup_bound(&memory, pc, context(0)),
            Err(RegistryError::CodeInvalidated)
        );
        assert_eq!(
            registry.lookup_bound(&memory, pc, context(0x4000)),
            Ok(based)
        );
        assert_eq!(
            registry.lookup_bound(&memory, pc, context(0x8000)),
            Err(RegistryError::NotFound { pc })
        );
        assert_eq!(
            registry.compile_bound(
                &memory,
                &specs,
                &[],
                CompileLimits::default(),
                7,
                Some(context(0x8000)),
                crate::cpu::x86::decode::DecodeAuthority::default()
            ),
            Err(RegistryError::UnitCapacity)
        );
        assert_eq!(registry.usage().units, 2);
    }
}

#[cfg(test)]
mod synthetic_key_tests {
    use super::*;
    use crate::{
        cpu::x86::{SegmentCache, SegmentContext},
        memory::{PageRange, Permissions},
    };

    fn context(base: u32) -> CodeFetchContext {
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = base;
        segments.caches[1] = SegmentCache::from_bits(0xf2b0_1000).unwrap();
        CodeFetchContext::from_segments(&segments).unwrap()
    }

    #[test]
    fn old_key_prefix_fault_does_not_own_new_keys_short_instruction_at_same_pc() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [0x1000, 0x5000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory.write(GuestAddress(0x1000), &[0x0f]).unwrap();
        memory.write(GuestAddress(0x5000), &[0xcc]).unwrap();
        let pc = GuestAddress(0x1000);
        let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
        let old = registry
            .compile_runtime_bound(
                &memory,
                pc,
                CompileLimits::default(),
                7,
                context(0),
                crate::cpu::x86::decode::DecodeAuthority::default(),
            )
            .unwrap();
        let fault = registry
            .get(&memory, old)
            .unwrap()
            .context_fault(pc.0)
            .unwrap();
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (2, 1)
        );
        assert!(!context(0x4000).permits(pc, fault.attempted_length));
        assert_eq!(
            registry.lookup_bound(&memory, pc, context(0x4000)),
            Err(RegistryError::NotFound { pc })
        );
        let new = registry
            .compile_runtime_bound(
                &memory,
                pc,
                CompileLimits::default(),
                7,
                context(0x4000),
                crate::cpu::x86::decode::DecodeAuthority::default(),
            )
            .unwrap();
        assert_ne!(old, new);
        assert_eq!(registry.lookup_bound(&memory, pc, context(0)), Ok(old));
        assert_eq!(registry.lookup_bound(&memory, pc, context(0x4000)), Ok(new));
        memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
        assert_eq!(registry.lookup_bound(&memory, pc, context(0x4000)), Ok(new));
    }
}

#[cfg(test)]
#[path = "resident/tests/paging.rs"]
mod paging_tests;

#[cfg(test)]
#[path = "resident/tests/cache/joins.rs"]
mod cache_join_tests;
