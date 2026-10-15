use std::collections::VecDeque;

use super::{
    ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
    initialization::{
        InitStep, InitializerCall32, LoadCompletion, ModuleOperation, OperationPurpose,
        PreparedInitializer, SavedModuleContext, load_steps,
    },
    lifetime::ModuleReference,
    tls::InitialThreadTls,
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{
            EXIT_SIZE, SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_state,
            encode_current_segment_context, encode_exit_v3, encode_state_current,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{SegmentRegister, State32},
    },
    loader::{ImageMetadata32, LoadPlan32, LoadedModule32, ModuleError32, ModuleId32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn start_module_process(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: ModuleRuntimeConfig32,
    ) -> Result<ModuleOperation32, HostError> {
        self.image_capacity()?;
        if decode_state(&self.arena()[..56]).map_err(|_| HostError::Infrastructure)?
            != State32::default()
            || !crate::process::cpu::context::ExtendedCpuState::initial()?.matches(self.arena())
        {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        if self.modules.loaded.iter().len() != 0 {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.load_module_request(name, actual_base, Some(config))
    }

    pub fn load_library_module(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        if self.modules.thread.is_none() {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        self.load_module_request(name, actual_base, None)
    }

    fn load_module_request(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let plan = self
            .modules
            .catalog
            .plan_load(&self.modules.loaded, self.memory()?, name, actual_base)
            .map_err(HostError::Module)?;
        self.install_module_plan(plan, config, LoadCompletion::Library)
    }

    pub fn resolve_module_export(
        &mut self,
        handle: u32,
        symbol: &crate::loader::ImportSymbol32,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let id = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        if let Some(resolved) = self
            .modules
            .loaded
            .direct_export_live(self.memory()?, id, symbol)
            .map_err(HostError::Module)?
        {
            return Ok(ModuleOperation32::ExportResolved {
                address: resolved.address().0,
            });
        }
        let (plan, resolved) = self
            .modules
            .catalog
            .plan_export_live(&self.modules.loaded, self.memory()?, id, symbol)
            .map_err(HostError::Module)?;
        self.install_module_plan(plan, None, LoadCompletion::Export(resolved.address().0))
    }

    fn install_module_plan(
        &mut self,
        mut plan: LoadPlan32,
        config: Option<ModuleRuntimeConfig32>,
        completion: LoadCompletion,
    ) -> Result<ModuleOperation32, HostError> {
        let initial = config.is_some();
        let root_id = plan.root();
        let root = plan
            .additions()
            .iter()
            .find(|module| module.id() == root_id)
            .or_else(|| self.modules.loaded.get(root_id))
            .ok_or(HostError::Infrastructure)?;
        if matches!(completion, LoadCompletion::Library) && root.is_dll() == initial {
            return Err(HostError::Module(ModuleError32::InvalidModuleKind));
        }
        if plan.additions().is_empty() {
            return match completion {
                LoadCompletion::Library => {
                    if !self.modules.attached.contains(&root_id) {
                        return Err(HostError::Module(ModuleError32::Busy));
                    }
                    let handle = root.base();
                    self.modules
                        .retain(root_id, false)
                        .map_err(HostError::Module)?;
                    Ok(ModuleOperation32::Loaded { handle })
                }
                LoadCompletion::Export(address) => {
                    plan.install(
                        self.memory.as_mut().ok_or(HostError::Closed)?,
                        &mut self.modules.loaded,
                    )
                    .map_err(HostError::Module)?;
                    Ok(ModuleOperation32::ExportResolved { address })
                }
            };
        }
        let initial_before = initial
            .then(|| SavedModuleContext::capture(self.arena()))
            .transpose()?;
        let slots = self
            .modules
            .catalog
            .limits()
            .modules
            .min(self.memory()?.capacity_pages() as usize) as u32;
        let mut new_thread = config
            .map(|config| InitialThreadTls::new(config, slots))
            .transpose()
            .map_err(HostError::Module)?;
        let thread = new_thread
            .as_ref()
            .or(self.modules.thread.as_ref())
            .ok_or(HostError::Infrastructure)?;
        for module in plan.additions() {
            let start = u64::from(module.base());
            let end = start + u64::from(module.image_size());
            if thread
                .reservations()
                .into_iter()
                .any(|(reserved_start, reserved_end)| start < reserved_end && reserved_start < end)
            {
                return Err(HostError::Module(ModuleError32::AddressConflict));
            }
        }
        let mut ready_arena = Vec::new();
        ready_arena
            .try_reserve_exact(self.arena().len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        ready_arena.extend_from_slice(self.arena());
        if initial {
            let selector = self.segment_context()?.state.selectors[SegmentRegister::Fs.index()];
            let descriptor = self
                .segment_descriptors
                .raw_descriptor(selector)
                .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
            if !descriptor.code_data() && self.segment_descriptors.descriptor(selector).is_none() {
                return Err(HostError::Module(ModuleError32::InvalidRequest));
            }
            let mut state = State32 {
                eip: root
                    .entry()
                    .ok_or(HostError::Module(ModuleError32::InvalidModuleKind))?
                    .0,
                ..State32::default()
            };
            state.registers[4] = thread.stack_end();
            encode_state_current(&state, &mut ready_arena[..56])
                .map_err(|_| HostError::Infrastructure)?;
            encode_exit_v3(
                &ExecutionExit {
                    retired: 0,
                    reason: ExitReason::NeedCode,
                },
                &mut ready_arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
            )
            .map_err(|_| HostError::Infrastructure)?;
            let mut context = self.segment_context()?;
            context.state.bases[SegmentRegister::Fs.index()] = thread.teb();
            let mut segments = [0; SEGMENTS_SIZE];
            let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
            encode_current_segment_context(&context, &mut segments, &mut bases)
                .map_err(|_| HostError::Infrastructure)?;
            ready_arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
                .copy_from_slice(&segments);
            ready_arena[SEGMENT_BASES_RECORD_OFFSET
                ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
                .copy_from_slice(&bases);
            thread.stage_runtime(&mut plan).map_err(HostError::Module)?;
        }
        let saved = SavedModuleContext::capture(&ready_arena)?;
        let (new_tls, mut writes) = thread
            .stage_modules(&mut plan, self.memory()?, initial)
            .map_err(HostError::Module)?;
        let mut steps =
            load_steps(plan.initialization_order(), initial).map_err(HostError::Module)?;
        if initial {
            for step in &mut steps {
                if let InitStep::DllMain { reserved, .. } = step {
                    *reserved = thread.teb();
                }
            }
        }
        let token = self
            .modules
            .next_initializer
            .checked_add(1)
            .ok_or(HostError::Module(ModuleError32::Capacity))?;
        let (prepared, attached) =
            prepare_first_initializer(&plan, &mut steps, &saved, token, thread.return_pc())?;
        if let Some(prepared) = &prepared {
            for write in prepared.frame.writes() {
                if initial {
                    plan.stage_write(write.address, &write.value.to_le_bytes())
                        .map_err(HostError::Module)?;
                } else {
                    writes.push(*write);
                }
            }
        }
        writes.sort_unstable_by_key(|write| write.address.0);
        self.modules
            .references
            .try_reserve(plan.additions().len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        self.modules
            .attached
            .try_reserve(plan.additions().len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        self.modules
            .operations
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        if let Some(thread) = new_thread.as_mut().or(self.modules.thread.as_mut()) {
            thread
                .allocations
                .try_reserve(new_tls.len())
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        }
        let loaded = plan
            .install_with_writes(
                self.memory.as_mut().ok_or(HostError::Closed)?,
                &mut self.modules.loaded,
                &writes,
            )
            .map_err(HostError::Module)?;
        for id in &loaded.new_ids {
            self.modules.references.push(ModuleReference {
                id: *id,
                explicit: 0,
                pinned: false,
            });
        }
        self.modules.attached.extend(attached);
        if let Some(thread) = new_thread {
            self.modules.thread = Some(thread);
        }
        self.modules
            .thread
            .as_mut()
            .unwrap()
            .allocations
            .extend(new_tls);
        if initial {
            let root = self.modules.loaded.get(root_id).unwrap();
            self.image = Some(ImageMetadata32 {
                image_base: root.base(),
                image_size: root.image_size(),
                entry_point: root.entry().unwrap().0,
                mapped_pages: root
                    .mapped_ranges()
                    .iter()
                    .map(|range| range.count as u32)
                    .sum(),
            });
            self.image_started = true;
            let selector = self.segment_context()?.state.selectors[SegmentRegister::Fs.index()];
            self.segment_descriptors
                .update_base(selector, self.modules.thread.as_ref().unwrap().teb())
                .expect("initial thread descriptor was validated before module publication");
        }
        let mut operation = ModuleOperation {
            purpose: OperationPurpose::Load {
                root: root_id,
                new_ids: loaded.new_ids,
                initial,
                completion,
                introduced_edges: loaded.introduced_edges,
            },
            steps,
            active: None,
            saved,
            outer: self.pending_call.take(),
            initial_before,
            last_returned: 0,
        };
        operation.saved.restore(self.arena.as_mut().get_mut());
        if let Some(prepared) = prepared {
            self.modules.next_initializer = prepared.descriptor.token;
            prepared.publish(self.arena.as_mut().get_mut());
            operation.active = Some(prepared.descriptor);
            self.modules.operations.push(operation);
            Ok(ModuleOperation32::Pending(prepared.descriptor))
        } else {
            self.complete_module_operation(operation)
        }
    }

    pub fn free_library_module(&mut self, handle: u32) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if !self.modules.operations.is_empty() || self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let root = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        let ids = self
            .modules
            .unreachable_after_release(root)
            .map_err(HostError::Module)?;
        if ids.is_empty() {
            self.modules
                .references
                .iter_mut()
                .find(|reference| reference.id == root)
                .unwrap()
                .explicit -= 1;
            return Ok(ModuleOperation32::Released);
        }
        let steps = self.detach_steps(&ids)?;
        let saved = SavedModuleContext::capture(self.arena())?;
        self.modules
            .operations
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let operation = ModuleOperation {
            purpose: OperationPurpose::Unload { root, ids },
            steps,
            active: None,
            saved,
            outer: self.pending_call.take(),
            initial_before: None,
            last_returned: 0,
        };
        let result = self.advance_module_operation(operation);
        if result.is_err()
            && self
                .modules
                .operations
                .last()
                .is_some_and(|operation| operation.last_returned == 0 && operation.active.is_none())
        {
            let operation = self.modules.operations.pop().unwrap();
            operation.saved.restore(self.arena.as_mut().get_mut());
            self.pending_call = operation.outer;
        }
        result
    }

    pub fn detach_modules_for_exit(&mut self) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if !self.modules.operations.is_empty() || self.callback.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(self.modules.attached.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        ids.extend_from_slice(&self.modules.attached);
        let mut steps = self.detach_steps(&ids)?;
        let reserved = self
            .modules
            .thread
            .as_ref()
            .map_or(1, |thread| thread.teb());
        for step in &mut steps {
            if let InitStep::DllMain {
                reserved: value, ..
            } = step
            {
                *value = reserved;
            }
        }
        self.modules
            .operations
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let operation = ModuleOperation {
            purpose: OperationPurpose::Shutdown,
            steps,
            active: None,
            saved: SavedModuleContext::capture(self.arena())?,
            outer: self.pending_call.take(),
            initial_before: None,
            last_returned: 0,
        };
        self.advance_module_operation(operation)
    }

    pub(super) fn detach_steps(&self, ids: &[ModuleId32]) -> Result<VecDeque<InitStep>, HostError> {
        let mut steps = VecDeque::new();
        steps
            .try_reserve(ids.len() * 2)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for module in self
            .modules
            .attached
            .iter()
            .rev()
            .filter(|module| ids.contains(module))
        {
            steps.push_back(InitStep::Tls {
                module: *module,
                reason: 0,
                index: 0,
            });
            steps.push_back(InitStep::DllMain {
                module: *module,
                reason: 0,
                reserved: 0,
            });
        }
        Ok(steps)
    }

    pub(super) fn prepare_module_rollback(
        &mut self,
        operation: &mut ModuleOperation,
        failed: Option<ModuleId32>,
    ) -> Result<(), HostError> {
        let OperationPurpose::Load {
            new_ids,
            initial,
            introduced_edges,
            ..
        } = &operation.purpose
        else {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        };
        let ids = self
            .modules
            .rollback_ids(new_ids, *initial, introduced_edges)
            .map_err(HostError::Module)?;
        let mut steps = self.detach_steps(&ids)?;
        if let Some(failed) =
            failed.filter(|failed| ids.contains(failed) && !self.modules.attached.contains(failed))
        {
            steps
                .try_reserve(2)
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            steps.push_front(InitStep::DllMain {
                module: failed,
                reason: 0,
                reserved: 0,
            });
            steps.push_front(InitStep::Tls {
                module: failed,
                reason: 0,
                index: 0,
            });
            self.modules.attached.push(failed);
        }
        let initial = *initial;
        let introduced_edges = match &mut operation.purpose {
            OperationPurpose::Load {
                introduced_edges, ..
            } => std::mem::take(introduced_edges),
            _ => unreachable!(),
        };
        operation.purpose = OperationPurpose::Rollback {
            ids,
            initial,
            introduced_edges,
        };
        operation.steps = steps;
        Ok(())
    }

    pub(super) fn complete_module_operation(
        &mut self,
        operation: ModuleOperation,
    ) -> Result<ModuleOperation32, HostError> {
        let result = (|| match &operation.purpose {
            OperationPurpose::Load {
                root,
                initial,
                completion,
                ..
            } => match completion {
                LoadCompletion::Library => {
                    self.modules
                        .retain(*root, *initial)
                        .map_err(HostError::Module)?;
                    let handle = self
                        .modules
                        .loaded
                        .get(*root)
                        .ok_or(HostError::Infrastructure)?
                        .base();
                    Ok(ModuleOperation32::Loaded { handle })
                }
                LoadCompletion::Export(address) => {
                    Ok(ModuleOperation32::ExportResolved { address: *address })
                }
            },
            OperationPurpose::Shutdown => {
                self.modules.attached.clear();
                Ok(ModuleOperation32::Released)
            }
            OperationPurpose::Unload { root, ids } => {
                self.commit_module_unload(ids, false, &[])?;
                if let Some(reference) = self
                    .modules
                    .references
                    .iter_mut()
                    .find(|reference| reference.id == *root)
                {
                    reference.explicit -= 1;
                }
                Ok(ModuleOperation32::Released)
            }
            OperationPurpose::Rollback {
                ids,
                initial,
                introduced_edges,
            } => {
                self.commit_module_unload(ids, *initial, introduced_edges)?;
                Ok(ModuleOperation32::InitializationFailed)
            }
        })();
        match result {
            Ok(result) => {
                if matches!(
                    operation.purpose,
                    OperationPurpose::Rollback { initial: true, .. }
                ) {
                    operation
                        .initial_before
                        .as_ref()
                        .ok_or(HostError::Infrastructure)?
                        .restore(self.arena.as_mut().get_mut());
                    self.segment_descriptors
                        .update_base(0x3b, 0)
                        .expect("default initial thread descriptor remains configured");
                    self.image = None;
                    self.image_started = false;
                } else {
                    operation.saved.restore(self.arena.as_mut().get_mut());
                }
                self.pending_call = operation.outer;
                self.complete_module_windows_initializer(result, operation.last_returned)?;
                Ok(result)
            }
            Err(error) => {
                self.modules.operations.push(operation);
                Err(error)
            }
        }
    }

    fn commit_module_unload(
        &mut self,
        ids: &[ModuleId32],
        initial: bool,
        introduced_edges: &[(ModuleId32, ModuleId32)],
    ) -> Result<(), HostError> {
        if ids.is_empty() && introduced_edges.is_empty() {
            return Ok(());
        }
        let mut plan = self
            .modules
            .loaded
            .plan_unload_rollback(self.memory()?, ids, introduced_edges)
            .map_err(HostError::Module)?;
        let thread = self
            .modules
            .thread
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        for allocation in thread
            .allocations
            .iter()
            .filter(|allocation| ids.contains(&allocation.module))
        {
            plan.add_range(allocation.range)
                .map_err(HostError::Module)?;
        }
        let writes = if initial {
            plan.add_range(
                PageRange::new(GuestAddress(thread.teb()), thread.runtime_pages())
                    .map_err(HostError::Memory)?,
            )
            .map_err(HostError::Module)?;
            plan.add_range(
                PageRange::new(
                    GuestAddress(thread.config.stack_base),
                    thread.config.stack_pages,
                )
                .map_err(HostError::Memory)?,
            )
            .map_err(HostError::Module)?;
            Vec::new()
        } else {
            thread.clear_writes(ids).map_err(HostError::Module)?
        };
        plan.unmap_with_writes(
            self.memory.as_mut().ok_or(HostError::Closed)?,
            &mut self.modules.loaded,
            &writes,
        )
        .map_err(HostError::Module)?;
        self.modules.remove_records(ids);
        if initial {
            self.modules.thread = None;
        }
        Ok(())
    }
}

fn prepare_first_initializer(
    plan: &LoadPlan32,
    steps: &mut VecDeque<InitStep>,
    saved: &SavedModuleContext,
    token: u64,
    return_pc: u32,
) -> Result<(Option<PreparedInitializer>, Vec<ModuleId32>), HostError> {
    let mut attached = Vec::new();
    attached
        .try_reserve_exact(plan.additions().len())
        .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
    loop {
        let Some(step) = steps.front() else {
            return Ok((None, attached));
        };
        let id = match *step {
            InitStep::Tls { module, .. }
            | InitStep::DllMain { module, .. }
            | InitStep::Attached(module) => module,
        };
        let module: &LoadedModule32 = plan
            .additions()
            .iter()
            .find(|module| module.id() == id)
            .ok_or(HostError::Infrastructure)?;
        let target = match *step {
            InitStep::Tls { reason, .. } => {
                if let Some(array) = module.tls().and_then(|tls| tls.callbacks_address) {
                    let mut bytes = [0; 4];
                    plan.read_staged(array, &mut bytes)
                        .map_err(HostError::Module)?;
                    let entry = u32::from_le_bytes(bytes);
                    (entry != 0).then_some((entry, ModuleInitializerKind32::TlsCallback, reason, 0))
                } else {
                    None
                }
            }
            InitStep::DllMain {
                reason, reserved, ..
            } if module.is_dll() => module
                .entry()
                .map(|entry| (entry.0, ModuleInitializerKind32::DllMain, reason, reserved)),
            InitStep::Attached(module) => {
                attached.push(module);
                None
            }
            _ => None,
        };
        if let Some((entry, kind, reason, reserved)) = target {
            let prepared = PreparedInitializer::prepare(
                token,
                module,
                InitializerCall32 {
                    entry,
                    kind,
                    reason,
                    reserved,
                },
                saved.state()?,
                return_pc,
            )?;
            if let Some(InitStep::Tls { index, .. }) = steps.front_mut() {
                *index += 1;
            } else {
                steps.pop_front();
            }
            return Ok((Some(prepared), attached));
        }
        steps.pop_front();
    }
}
