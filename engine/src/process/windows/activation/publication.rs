use super::{
    ActivationRuntime32, memory, sources::CreateWork32, storage::PreparedContextStorage32,
};
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange},
    process::{EngineInstance, HostError},
    windows::activation::{
        ActivationError32 as E, ActivationSectionKey32, encode_activation_sections,
    },
};

impl EngineInstance {
    pub(super) fn publish_activation_context(
        &mut self,
        work: &CreateWork32,
    ) -> Result<u32, HostError> {
        let runtime = self
            .activation
            .as_ref()
            .ok_or(HostError::Activation(E::InvalidParameter))?;
        let graph = work.graph.as_ref().ok_or(HostError::Activation(E::Stale))?;
        let prepared = runtime
            .native
            .prepare_create_shared(graph.clone(), work.specification.process_default)
            .map_err(HostError::Activation)?;
        let budget = (runtime.config.section_bytes as usize)
            .checked_sub(runtime.used)
            .ok_or(HostError::Activation(E::NotEnoughMemory))?;
        let sections = encode_activation_sections(graph, prepared.handle(), budget)
            .map_err(HostError::Activation)?;
        let storage = PreparedContextStorage32::prepare(
            self.memory()?,
            runtime.config.section_bounds,
            prepared.handle(),
            sections,
            work.source_cost,
            budget,
        )
        .map_err(HostError::Activation)?;
        runtime
            .native
            .can_commit_create(&prepared)
            .map_err(HostError::Activation)?;
        self.check_close_provider_files(&work.leases)?;
        let storage = storage
            .publish(self.memory.as_mut().unwrap())
            .map_err(HostError::Activation)?;
        self.commit_close_provider_files(&work.leases);
        let runtime = self.activation.as_mut().unwrap();
        let handle = runtime.native.commit_create(prepared);
        runtime.used += storage.cost;
        runtime.storage.push(storage);
        Ok(handle)
    }
}
impl ActivationRuntime32 {
    pub(super) fn activate(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        handle: u32,
        output: u32,
    ) -> Result<u32, E> {
        memory::output(memory, output, 4)?;
        if handle != 0 {
            self.storage(handle)?.check(memory)?;
        }
        let prepared = self.native.prepare_activate(thread, handle)?;
        self.native.can_commit_activate(&prepared)?;
        memory
            .write(GuestAddress(output), &prepared.cookie().to_le_bytes())
            .map_err(|_| E::NoAccess)?;
        self.native.commit_activate(prepared);
        Ok(1)
    }
    pub(super) fn current(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        output: u32,
    ) -> Result<u32, E> {
        memory::output(memory, output, 4)?;
        let handle = self.native.current(thread);
        if handle != 0 {
            self.storage(handle)?.check(memory)?;
        }
        self.native.can_add_ref(handle)?;
        memory
            .write(GuestAddress(output), &handle.to_le_bytes())
            .map_err(|_| E::NoAccess)?;
        self.native
            .add_ref(handle)
            .expect("current activation reference was preflighted");
        Ok(1)
    }
    pub(super) fn release(&mut self, memory: &mut AddressSpace, handle: u32) -> Result<u32, E> {
        let disposed = self.native.can_release(handle)?;
        if disposed {
            let ranges = self.disposal_ranges(memory, &[handle])?;
            if !ranges.is_empty() {
                memory
                    .unmap_ranges_with_word_writes(&ranges, &[])
                    .map_err(|_| E::NoAccess)?;
            }
        }
        self.native.release(handle)?;
        if disposed {
            self.retire_storage(&[handle]);
        }
        Ok(0)
    }
    pub(super) fn deactivate(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        flags: u32,
        cookie: u32,
    ) -> Result<u32, E> {
        let prepared = self.native.prepare_deactivate(thread, flags, cookie)?;
        let disposed = self.native.disposed_after_deactivate(&prepared)?;
        let ranges = self.disposal_ranges(memory, &disposed)?;
        if !ranges.is_empty() {
            memory
                .unmap_ranges_with_word_writes(&ranges, &[])
                .map_err(|_| E::NoAccess)?;
        }
        self.native.commit_deactivate(prepared);
        self.retire_storage(&disposed);
        Ok(1)
    }
    pub(super) fn disposal_ranges(
        &self,
        memory: &AddressSpace,
        handles: &[u32],
    ) -> Result<Vec<PageRange>, E> {
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(handles.len())
            .map_err(|_| E::NotEnoughMemory)?;
        for &handle in handles {
            let storage = self.storage(handle)?;
            storage.check(memory)?;
            if let Some(range) = storage.range {
                ranges.push(range);
            }
        }
        Ok(ranges)
    }
    pub(super) fn find_section(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        args: &[u32],
        key: ActivationSectionKey32,
    ) -> Result<u32, E> {
        if args[0] & !1 != 0 || args[1] != 0 || args[4] == 0 {
            return Err(E::InvalidParameter);
        }
        let size = u32::from_le_bytes(memory::read(memory, args[4])?);
        if size < 36 {
            return Err(E::InvalidParameter);
        }
        let output_size = size.min(64) as usize;
        memory::output(memory, args[4], output_size)?;
        let valid = match &key {
            ActivationSectionKey32::String(_) => matches!(args[2], 1 | 2 | 3 | 7),
            ActivationSectionKey32::Guid(_) => matches!(args[2], 4 | 5 | 6 | 9),
        };
        if !valid {
            return Err(E::SectionNotFound);
        }
        let mut found = None;
        for handle in self.native.effective_lookup_handles(thread) {
            if handle == 0 {
                continue;
            }
            self.native.graph(handle)?;
            let storage = self.storage(handle)?;
            storage.check(memory)?;
            if let Some(section) = storage
                .sections
                .iter()
                .find(|section| section.section.id == args[2])
                && let Some(entry) = section.section.find(&key)
            {
                let base = storage.base() + section.offset;
                found = Some((
                    handle,
                    [
                        size,
                        1,
                        base + entry.offset,
                        entry.length,
                        if section.section.global_length == 0 {
                            0
                        } else {
                            base + section.section.global_offset
                        },
                        section.section.global_length,
                        base,
                        section.section.bytes.len() as u32,
                        if args[0] & 1 != 0 { handle } else { 0 },
                        entry.roster,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                    ],
                ));
                break;
            }
        }
        let (handle, words) = found.ok_or(E::KeyNotFound)?;
        if args[0] & 1 != 0 {
            self.native.can_add_ref(handle)?;
        }
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        memory
            .write(GuestAddress(args[4]), &bytes[..output_size])
            .map_err(|_| E::NoAccess)?;
        if args[0] & 1 != 0 {
            self.native
                .add_ref(handle)
                .expect("returned activation reference was preflighted");
        }
        Ok(1)
    }
}
