use crate::{
    memory::{Access, GuestAddress, WordWrite32},
    process::{EngineInstance, HostError},
    windows::synchronization::{
        INFINITE, SynchronizationApi32 as Api, SynchronizationError32 as Error, WaitMode32,
        WaitOutcome32, WaitResult32,
    },
};

pub(super) struct CriticalSection32 {
    address: GuestAddress,
    handle: u32,
    debug: u32,
    mappings: [(GuestAddress, u64); 2],
    mapping_count: usize,
}
impl EngineInstance {
    pub(super) fn execute_critical_call(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<WaitOutcome32, HostError> {
        let address = GuestAddress(args[0]);
        let ready = |value| {
            Ok(WaitOutcome32::Ready(WaitResult32 {
                value,
                last_error: None,
            }))
        };
        if matches!(
            api,
            Api::InitializeCriticalSection
                | Api::InitializeCriticalSectionAndSpinCount
                | Api::InitializeCriticalSectionEx
        ) {
            let flags = if api == Api::InitializeCriticalSectionEx {
                args[2]
            } else {
                0
            };
            if flags & !0x0100_0000 != 0 {
                return Err(HostError::Synchronization(Error::InvalidWait));
            }
            self.initialize_critical(address, flags != 0)?;
            return ready(u32::from(api != Api::InitializeCriticalSection));
        }
        let index = self.check_critical(address)?;
        let handle = self.synchronization.as_ref().unwrap().critical[index].handle;
        match api {
            Api::EnterCriticalSection | Api::TryEnterCriticalSection => {
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
                if self
                    .synchronization_mut()?
                    .try_acquire_mutex(handle)
                    .map_err(HostError::Synchronization)?
                {
                    self.publish_critical_state(address);
                    return ready(u32::from(api == Api::TryEnterCriticalSection));
                }
                if api == Api::TryEnterCriticalSection {
                    return ready(0);
                }
                let outcome = self
                    .synchronization_mut()?
                    .wait(&[handle], WaitMode32::Any, INFINITE, false, None)
                    .map_err(HostError::Synchronization)?;
                self.publish_critical_state(address);
                Ok(outcome)
            }
            Api::LeaveCriticalSection => {
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
                let context = self.synchronization()?;
                let prepared = context
                    .prepare_release_mutex(handle)
                    .map_err(HostError::Synchronization)?;
                let (owner, depth) = prepared.owner_depth();
                let current = self.threads.current_id();
                let waiters = context
                    .object_waiters(handle)
                    .map_err(HostError::Synchronization)?;
                let waiters = waiters.saturating_sub(usize::from(owner != 0 && owner != current));
                self.write_critical_state(address, handle, owner, depth, waiters);
                self.synchronization_mut()?.commit_release_mutex(prepared);
                ready(0)
            }
            Api::SetCriticalSectionSpinCount => ready(0),
            Api::DeleteCriticalSection => {
                let context = self.synchronization()?;
                if context
                    .mutex_state(handle)
                    .map_err(HostError::Synchronization)?
                    .0
                    != 0
                    || context
                        .object_waiters(handle)
                        .map_err(HostError::Synchronization)?
                        != 0
                {
                    return Err(HostError::Synchronization(Error::Busy));
                }
                let prepared = context
                    .prepare_close_handles(&[handle])
                    .map_err(HostError::Synchronization)?;
                let debug = self.synchronization.as_ref().unwrap().critical[index].debug;
                if debug != u32::MAX {
                    self.check_windows_runtime_bytes(debug, 32)?;
                }
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(address, &[0; 24])
                    .map_err(HostError::Memory)?;
                self.synchronization_mut()?.commit_close_handles(prepared);
                if debug != u32::MAX {
                    self.free_windows_runtime_bytes(debug);
                }
                self.synchronization
                    .as_mut()
                    .unwrap()
                    .critical
                    .swap_remove(index);
                ready(0)
            }
            _ => Err(HostError::Synchronization(Error::Unsupported)),
        }
    }
    fn initialize_critical(
        &mut self,
        address: GuestAddress,
        no_debug: bool,
    ) -> Result<(), HostError> {
        self.memory()?
            .check_span(address, 24, Access::Write)
            .map_err(HostError::Memory)?;
        let end = u64::from(address.0) + 24;
        let runtime = self.synchronization.as_ref().unwrap();
        if runtime.critical.iter().any(|section| {
            u64::from(section.address.0) < end
                && u64::from(address.0) < u64::from(section.address.0) + 24
        }) {
            return Err(HostError::Synchronization(Error::Busy));
        }
        if runtime.critical.len() == runtime.critical.capacity() {
            return Err(HostError::Synchronization(Error::Capacity));
        }
        let (mutex, _) = runtime
            .context
            .prepare_mutex(false, false, None)
            .map_err(HostError::Synchronization)?;
        let handle = mutex.handle();
        let mut mappings = [(GuestAddress(0), 0); 2];
        let first = address.0 & !4095;
        let last = (address.0 + 23) & !4095;
        let mapping_count = if first == last { 1 } else { 2 };
        for (index, page) in [first, last].into_iter().take(mapping_count).enumerate() {
            mappings[index] = (
                GuestAddress(page),
                self.memory()?
                    .mapping_version_at(GuestAddress(page))
                    .map_err(HostError::Memory)?,
            );
        }
        self.memory()?
            .ensure_transaction_versions(if no_debug { 1 } else { 2 })
            .map_err(HostError::Memory)?;
        let debug = if no_debug {
            u32::MAX
        } else {
            self.allocate_windows_runtime_bytes(32)?
        };
        let mut words = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; 14];
        for (index, value) in [debug, u32::MAX, 0, 0, handle, 0].into_iter().enumerate() {
            words[index] = WordWrite32 {
                address: GuestAddress(address.0 + index as u32 * 4),
                value,
            };
        }
        let count = if no_debug {
            6
        } else {
            for index in 0..8 {
                words[index + 6] = WordWrite32 {
                    address: GuestAddress(debug + index as u32 * 4),
                    value: match index {
                        1 => address.0,
                        2 | 3 => debug + 8,
                        _ => 0,
                    },
                };
            }
            14
        };
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(&words[..count])
            .expect("critical section output and debug heap allocation preflighted");
        self.synchronization_mut()?.commit_object_handle(mutex);
        self.synchronization
            .as_mut()
            .unwrap()
            .critical
            .push(CriticalSection32 {
                address,
                handle,
                debug,
                mappings,
                mapping_count,
            });
        Ok(())
    }
    pub(super) fn check_critical(&self, address: GuestAddress) -> Result<usize, HostError> {
        let runtime = self.synchronization.as_ref().unwrap();
        let index = runtime
            .critical
            .iter()
            .position(|section| section.address == address)
            .ok_or(HostError::Synchronization(Error::InvalidWait))?;
        let section = &runtime.critical[index];
        for &(page, version) in &section.mappings[..section.mapping_count] {
            if self.memory()?.mapping_version_at(page) != Ok(version) {
                return Err(HostError::Synchronization(Error::Stale));
            }
        }
        self.memory()?
            .check_span(address, 24, Access::Write)
            .map_err(HostError::Memory)?;
        Ok(index)
    }
    pub(super) fn publish_critical_state(&mut self, address: GuestAddress) {
        let runtime = self.synchronization.as_ref().unwrap();
        let section = runtime
            .critical
            .iter()
            .find(|section| section.address == address)
            .unwrap();
        let (owner, depth) = runtime
            .context
            .mutex_state(section.handle)
            .expect("critical section retains its private mutex");
        let waiters = runtime.context.object_waiters(section.handle).unwrap();
        self.write_critical_state(address, section.handle, owner, depth, waiters);
    }
    fn write_critical_state(
        &mut self,
        address: GuestAddress,
        handle: u32,
        owner: u32,
        depth: u32,
        waiters: usize,
    ) {
        let count = i64::from(depth) + waiters as i64 - 1;
        let words = [
            WordWrite32 {
                address: GuestAddress(address.0 + 4),
                value: count as u32,
            },
            WordWrite32 {
                address: GuestAddress(address.0 + 8),
                value: depth,
            },
            WordWrite32 {
                address: GuestAddress(address.0 + 12),
                value: owner,
            },
            WordWrite32 {
                address: GuestAddress(address.0 + 16),
                value: handle,
            },
            WordWrite32 {
                address: GuestAddress(address.0 + 20),
                value: 0,
            },
        ];
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(&words)
            .expect("critical section publication was preflighted");
    }
}
