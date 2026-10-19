use super::{ArtifactError, RegistryError, ResidentRegistry, UnitId};
use crate::{
    cpu::x86::decode::{CodeDecodeKey, CodeFetchContext},
    memory::{AddressSpace, GuestAddress},
};

impl ResidentRegistry {
    pub fn lookup(&self, memory: &AddressSpace, pc: GuestAddress) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        let mut stale = false;
        for &id in self.entry_index.candidates(pc.0) {
            let unit = &self.entries[self.unit_index(id.get())?];
            if unit.region.matches_decode_key(CodeDecodeKey::FLAT) {
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
        let candidates = self.entry_index.candidates(pc.0);
        for material in [true, false] {
            for &id in candidates {
                let unit = &self.entries[self.unit_index(id.get())?];
                if !unit.region.matches_decode_key(cs.decode_key()) {
                    continue;
                }
                let fault = unit.region.context_fault(pc.0);
                // exact index membership is material or a fault start; only a
                // fault at this pc leaves material membership ambiguous.
                let matches = if material {
                    fault.is_none() || unit.region.contains_material(pc.0)
                } else {
                    fault.is_some()
                };
                if matches {
                    match unit.region.wasm_bytes(memory) {
                        Ok(_)
                            if material
                                || fault.is_some_and(|fault| fault.remains_denied(memory, cs)) =>
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
}
