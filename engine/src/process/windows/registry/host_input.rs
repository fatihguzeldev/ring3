use super::*;
use crate::process::{EngineInstance, HostError};

pub(super) struct RegistrySeedInput32 {
    pub token: u64,
    pub total: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy)]
pub struct RegistryInput32 {
    pub epoch: u64,
    pub token: u64,
    pub received: u32,
    pub total: u32,
}
impl EngineInstance {
    pub fn begin_registry_seed(&mut self, total: u32) -> Result<RegistryInput32, HostError> {
        self.memory()?;
        let runtime = self
            .registry
            .as_mut()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?;
        if runtime.input.is_some() {
            return Err(HostError::Registry(RegistryError32::Busy));
        }
        // Three bounded UTF-16 names plus the configured value budget.
        let maximum = u64::from(runtime.native.limits().bytes) + 3 * 65536 + 64;
        if total < 64 || u64::from(total) > maximum {
            return Err(HostError::Registry(RegistryError32::InvalidParameter));
        }
        let next = runtime
            .next_input
            .checked_add(1)
            .ok_or(HostError::Registry(RegistryError32::NotEnoughMemory))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total as usize)
            .map_err(|_| HostError::Registry(RegistryError32::NotEnoughMemory))?;
        let token = runtime.next_input;
        runtime.input = Some(RegistrySeedInput32 {
            token,
            total,
            bytes,
        });
        runtime.next_input = next;
        Ok(RegistryInput32 {
            epoch: runtime.native.epoch(),
            token,
            received: 0,
            total,
        })
    }
    pub fn append_registry_seed(
        &mut self,
        epoch: u64,
        token: u64,
        offset: u32,
        bytes: &[u8],
    ) -> Result<RegistryInput32, HostError> {
        self.memory()?;
        let runtime = self
            .registry
            .as_mut()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?;
        if runtime.native.epoch() != epoch {
            return Err(HostError::Registry(RegistryError32::StaleReceipt));
        }
        let input = runtime
            .input
            .as_mut()
            .filter(|input| input.token == token)
            .ok_or(HostError::Registry(RegistryError32::StaleReceipt))?;
        if bytes.is_empty()
            || bytes.len() > 4096
            || offset as usize != input.bytes.len()
            || input
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > input.total as usize)
        {
            return Err(HostError::Registry(RegistryError32::InvalidParameter));
        }
        input.bytes.extend_from_slice(bytes);
        Ok(RegistryInput32 {
            epoch,
            token,
            received: input.bytes.len() as u32,
            total: input.total,
        })
    }
    pub fn finish_registry_seed(&mut self, epoch: u64, token: u64) -> Result<(), HostError> {
        self.memory()?;
        let runtime = self
            .registry
            .as_ref()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?;
        let input = runtime
            .input
            .as_ref()
            .filter(|input| input.token == token && runtime.native.epoch() == epoch)
            .ok_or(HostError::Registry(RegistryError32::StaleReceipt))?;
        let bytes = &input.bytes;
        let invalid = || HostError::Registry(RegistryError32::InvalidData);
        if bytes.len() != input.total as usize
            || bytes.len() < 64
            || &bytes[..8] != b"R3RS\x01\0\x01\0"
            || word(bytes, 8) != input.total
            || word(bytes, 12) != 0
            || bytes[56..64].iter().any(|&v| v != 0)
        {
            return Err(invalid());
        }
        let root = word(bytes, 16);
        let view = match word(bytes, 20) {
            32 => RegistryView32::View32,
            64 => RegistryView32::View64,
            _ => return Err(invalid()),
        };
        let lengths = [word(bytes, 24), word(bytes, 28), word(bytes, 32)];
        if lengths.iter().any(|&v| v > 32767) || word(bytes, 52) > 1 {
            return Err(invalid());
        }
        let text_bytes = lengths
            .iter()
            .try_fold(0usize, |n, &v| n.checked_add(v as usize * 2))
            .ok_or_else(invalid)?;
        if 64usize
            .checked_add(text_bytes)
            .and_then(|n| n.checked_add(word(bytes, 40) as usize))
            != Some(bytes.len())
        {
            return Err(invalid());
        }
        let mut names = Vec::new();
        names
            .try_reserve_exact(text_bytes / 2)
            .map_err(|_| HostError::Registry(RegistryError32::NotEnoughMemory))?;
        names.extend(
            bytes[64..64 + text_bytes]
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes([v[0], v[1]])),
        );
        let p = lengths[0] as usize;
        let n = p + lengths[1] as usize;
        let modified = u64::from_le_bytes(bytes[44..52].try_into().unwrap());
        let kind = word(bytes, 36);
        let volatile = word(bytes, 52) != 0;
        // Snapshot payload before borrowing the runtime mutably; admission remains transactional.
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len() - 64 - text_bytes)
            .map_err(|_| HostError::Registry(RegistryError32::NotEnoughMemory))?;
        data.extend_from_slice(&bytes[64 + text_bytes..]);
        self.registry_seed(RegistrySeed32 {
            root,
            view,
            path: &names[..p],
            name: &names[p..n],
            class: &names[n..],
            kind,
            bytes: &data,
            modified,
            volatile,
        })?;
        self.registry.as_mut().unwrap().input = None;
        Ok(())
    }
    pub fn abort_registry_seed(&mut self, epoch: u64, token: u64) -> Result<(), HostError> {
        self.memory()?;
        let runtime = self
            .registry
            .as_mut()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?;
        if runtime.native.epoch() != epoch
            || !runtime
                .input
                .as_ref()
                .is_some_and(|input| input.token == token)
        {
            return Err(HostError::Registry(RegistryError32::StaleReceipt));
        }
        runtime.input = None;
        Ok(())
    }
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
