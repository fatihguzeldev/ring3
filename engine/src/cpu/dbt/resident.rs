use std::sync::atomic::{AtomicU64, Ordering};

use crate::memory::{AddressSpace, GuestAddress};

use super::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, CompiledRegion, artifact::emit_prepared,
    compile_region, region::prepare_resident_read_region, wasm::EmbeddedBinding,
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
        let wasm_bytes = self.admit(memory, &region)?;
        let id = allocate_id(&NEXT_UNIT_ID)?;
        self.publish(id, region, wasm_bytes);
        Ok(id)
    }

    pub(crate) fn compile_bound(
        &mut self,
        memory: &AddressSpace,
        specs: &[BlockSpec],
        limits: CompileLimits,
        key: u64,
    ) -> Result<UnitId, RegistryError> {
        self.compile_bound_with_counter(memory, specs, limits, key, &NEXT_UNIT_ID)
    }

    fn compile_bound_with_counter(
        &mut self,
        memory: &AddressSpace,
        specs: &[BlockSpec],
        limits: CompileLimits,
        key: u64,
        counter: &AtomicU64,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        if self.entries.len() == self.limits.units {
            return Err(RegistryError::UnitCapacity);
        }
        let prepared =
            prepare_resident_read_region(memory, specs, limits).map_err(RegistryError::Compile)?;
        // final emission can fail after reservation; unpublished ids are never reused.
        let id = allocate_id(counter)?;
        let region = emit_prepared(
            prepared,
            limits,
            Some(EmbeddedBinding::Resident { key, id: id.get() }),
        )
        .map_err(RegistryError::Compile)?;
        let wasm_bytes = self.admit(memory, &region)?;
        self.publish(id, region, wasm_bytes);
        Ok(id)
    }

    fn admit(
        &self,
        memory: &AddressSpace,
        region: &CompiledRegion,
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
        for pc in region.instruction_addresses() {
            if self.entries.iter().any(|unit| {
                unit.region.contains_instruction(pc.0) && unit.region.wasm_bytes(memory).is_ok()
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
                CompileLimits::default(),
                7,
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
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                7,
                &counter
            ),
            Err(RegistryError::Compile(CompileError::WasmLimit))
        );
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(
            registry.compile_bound_with_counter(
                &memory,
                &specs(0x1000),
                CompileLimits::default(),
                7,
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
                CompileLimits::default(),
                7,
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
                CompileLimits::default(),
                7,
                &counter,
            )
            .unwrap();
        let usage = tight.usage();
        assert_eq!(
            tight.compile_bound_with_counter(
                &memory,
                &specs(0x2000),
                CompileLimits::default(),
                7,
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
                CompileLimits {
                    blocks: 0,
                    ..CompileLimits::default()
                },
                7,
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
