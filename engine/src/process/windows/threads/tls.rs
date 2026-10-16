use super::*;
use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, HostError},
};

const TLS_SLOTS: usize = 1088;
const INLINE_SLOTS: usize = 64;
const INLINE_OFFSET: u32 = 0xe10;
const EXPANSION_OFFSET: u32 = 0xf94;

impl ThreadRuntime32 {
    fn check_tls_index(&self, index: u32) -> Result<usize, ThreadError32> {
        let index = index as usize;
        if index >= TLS_SLOTS || self.tls_allocated[index / 64] & (1 << (index % 64)) == 0 {
            return Err(ThreadError32::InvalidParameter);
        }
        Ok(index)
    }
    fn current_record(&self) -> &ThreadRecord32 {
        self.threads
            .iter()
            .find(|thread| thread.id == self.current)
            .expect("current thread exists")
    }
    fn current_record_mut(&mut self) -> &mut ThreadRecord32 {
        self.threads
            .iter_mut()
            .find(|thread| thread.id == self.current)
            .expect("current thread exists")
    }
}

impl EngineInstance {
    pub(in crate::process) fn allocate_windows_tls(&mut self) -> Result<u32, HostError> {
        self.memory()?;
        let Some(index) = (0..TLS_SLOTS)
            .find(|index| self.threads.tls_allocated[index / 64] & (1 << (index % 64)) == 0)
        else {
            return Ok(u32::MAX);
        };
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(self.threads.threads.len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        for thread in &self.threads.threads {
            if thread.state == ThreadState32::Terminated {
                continue;
            }
            if let Some(teb) = thread.teb {
                if let Some(address) = tls_address(self.memory()?, teb, index)? {
                    writes.push(WordWrite32 { address, value: 0 });
                }
            }
        }
        self.memory
            .as_mut()
            .unwrap()
            .write_thread_words32(&writes)
            .map_err(HostError::Memory)?;
        for thread in &mut self.threads.threads {
            if index < thread.tls_values.len() {
                thread.tls_values[index] = 0;
            }
        }
        self.threads.tls_allocated[index / 64] |= 1 << (index % 64);
        Ok(index as u32)
    }
    pub(in crate::process) fn free_windows_tls(&mut self, index: u32) -> Result<(), HostError> {
        self.memory()?;
        let index = self
            .threads
            .check_tls_index(index)
            .map_err(HostError::Thread)?;
        self.threads.tls_allocated[index / 64] &= !(1 << (index % 64));
        Ok(())
    }
    pub(in crate::process) fn get_windows_tls(&self, index: u32) -> Result<u32, HostError> {
        self.memory()?;
        let index = self
            .threads
            .check_tls_index(index)
            .map_err(HostError::Thread)?;
        let thread = self.threads.current_record();
        if let Some(teb) = thread.teb {
            if let Some(address) = tls_address(self.memory()?, teb, index)? {
                return read_word(self.memory()?, address);
            }
            return Ok(0);
        }
        Ok(thread.tls_values.get(index).copied().unwrap_or(0))
    }
    pub(in crate::process) fn set_windows_tls(
        &mut self,
        index: u32,
        value: u32,
    ) -> Result<(), HostError> {
        self.memory()?;
        let index = self
            .threads
            .check_tls_index(index)
            .map_err(HostError::Thread)?;
        if let Some(teb) = self.threads.current_teb() {
            if let Some(address) = tls_address(self.memory()?, teb, index)? {
                return self
                    .memory
                    .as_mut()
                    .unwrap()
                    .write_words32(&[WordWrite32 { address, value }])
                    .map_err(HostError::Memory);
            }
            self.threads
                .current_record_mut()
                .reserved
                .try_reserve(1)
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            let external = self.guest_address_reservations()?;
            let base = self
                .windows_memory
                .reserve_thread_storage(self.memory.as_mut().unwrap(), &external, 4096)
                .map_err(HostError::WindowsMemory)?;
            let range = PageRange::new(GuestAddress(base), 1).unwrap();
            let pointer = base.to_le_bytes();
            let value = value.to_le_bytes();
            let result = self.windows_memory.commit_thread_storage(
                self.memory.as_mut().unwrap(),
                base,
                4096,
                &[
                    (GuestAddress(teb + EXPANSION_OFFSET), &pointer),
                    (
                        GuestAddress(base + (index - INLINE_SLOTS) as u32 * 4),
                        &value,
                    ),
                ],
            );
            if let Err(error) = result {
                self.windows_memory.revoke(range);
                return Err(HostError::WindowsMemory(error));
            }
            let thread = self.threads.current_record_mut();
            thread.tls_expansion = Some(range);
            thread.reserved.push(range);
            return Ok(());
        }
        let thread = self.threads.current_record_mut();
        if thread.tls_values.len() <= index {
            thread
                .tls_values
                .try_reserve_exact(TLS_SLOTS - thread.tls_values.len())
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            thread.tls_values.resize(TLS_SLOTS, 0);
        }
        thread.tls_values[index] = value;
        Ok(())
    }
}

fn tls_address(
    memory: &crate::memory::AddressSpace,
    teb: u32,
    index: usize,
) -> Result<Option<GuestAddress>, HostError> {
    if index < INLINE_SLOTS {
        return Ok(Some(GuestAddress(teb + INLINE_OFFSET + index as u32 * 4)));
    }
    let base = read_word(memory, GuestAddress(teb + EXPANSION_OFFSET))?;
    if base == 0 {
        return Ok(None);
    }
    let address = base
        .checked_add((index - INLINE_SLOTS) as u32 * 4)
        .ok_or(HostError::Thread(ThreadError32::NoAccess))?;
    Ok(Some(GuestAddress(address)))
}

fn read_word(
    memory: &crate::memory::AddressSpace,
    address: GuestAddress,
) -> Result<u32, HostError> {
    let mut bytes = [0; 4];
    memory
        .read(address, &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(u32::from_le_bytes(bytes))
}
