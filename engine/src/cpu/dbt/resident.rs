use std::sync::atomic::{AtomicU64, Ordering};

use crate::memory::{AddressSpace, GuestAddress};

use super::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, CompiledRegion, compile_region,
};

const MAX_UNITS: usize = 8;
const MAX_BYTES: usize = MAX_UNITS * 65_536;
static NEXT_UNIT_ID: AtomicU64 = AtomicU64::new(1);

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
        if !(1..=MAX_UNITS).contains(&limits.units) || !(1..=MAX_BYTES).contains(&limits.wasm_bytes)
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
        let bytes = region
            .wasm_bytes(memory)
            .map_err(|ArtifactError::CodeInvalidated| RegistryError::CodeInvalidated)?
            .len();
        let wasm_bytes = self
            .wasm_bytes
            .checked_add(bytes)
            .filter(|&total| total <= self.limits.wasm_bytes)
            .ok_or(RegistryError::ByteCapacity)?;
        for pc in region.instruction_addresses() {
            if self.entries.iter().any(|unit| {
                unit.region.contains_instruction(pc.0) && unit.region.wasm_bytes(memory).is_ok()
            }) {
                return Err(RegistryError::InstructionOverlap { pc });
            }
        }
        let id = allocate_id(&NEXT_UNIT_ID)?;
        // the reserved vector makes publication infallible after the id claim.
        self.entries.push(ResidentUnit { id, region });
        self.wasm_bytes = wasm_bytes;
        Ok(id)
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
            if unit.region.contains_instruction(pc.0) {
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

    pub fn usage(&self) -> RegistryUsage {
        RegistryUsage {
            units: self.entries.len(),
            wasm_bytes: self.wasm_bytes,
        }
    }

    fn check_memory(&self, memory: &AddressSpace) -> Result<(), RegistryError> {
        if memory.identity() != self.memory_identity {
            return Err(RegistryError::WrongAddressSpace);
        }
        Ok(())
    }
}

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
