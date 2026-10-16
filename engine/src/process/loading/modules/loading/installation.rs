use std::collections::VecDeque;

use super::super::{
    ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
    initialization::{
        InitStep, InitialModuleContext, InitializerCall32, LoadCompletion, ModuleOperation,
        OperationPurpose, PreparedInitializer, SavedModuleContext, load_steps,
    },
    lifetime::ModuleReference,
    tls::InitialThreadTls,
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{
            EXIT_SIZE, SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context,
            encode_exit_v3, encode_state_current,
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
    pub(in crate::process::loading::modules) fn install_module_plan_ready(
        &mut self,
        mut plan: LoadPlan32,
        config: Option<ModuleRuntimeConfig32>,
        completion: LoadCompletion,
        activation_work: &[crate::process::windows::activation::modules::ModuleActivationWork32],
    ) -> Result<ModuleOperation32, HostError> {
        let initial = config.is_some();
        let root_id = plan.root();
        let root = plan
            .additions()
            .iter()
            .find(|module| module.id() == root_id)
            .or_else(|| self.modules.loaded.get(root_id))
            .ok_or(HostError::Infrastructure)?;
        if matches!(completion, LoadCompletion::Library) && initial && root.is_dll() {
            return Err(HostError::Module(ModuleError32::InvalidModuleKind));
        }
        if plan.additions().is_empty() {
            return match completion {
                LoadCompletion::Library => {
                    if !self.modules.attached.contains(&root_id) && !root.is_uninitialized() {
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
        let startup_image_base = root.base();
        let startup_image_name = root.identity().to_owned();
        let initial_before = initial
            .then(|| InitialModuleContext::capture(self))
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
        let prepared_startup = if initial {
            Some(self.stage_process_startup(
                &mut plan,
                thread.teb(),
                thread.reservations(),
                startup_image_base,
                &startup_image_name,
            )?)
        } else {
            None
        };
        let prepared_memory = if let Some(startup) = &prepared_startup {
            let mut reservations = self.guest_address_reservations()?;
            reservations.push(startup.range);
            for module in plan.additions() {
                reservations.push(
                    PageRange::new(
                        GuestAddress(module.base()),
                        module.image_size().div_ceil(crate::memory::PAGE_SIZE),
                    )
                    .map_err(HostError::Memory)?,
                );
            }
            for (start, end) in thread.reservations() {
                reservations.push(
                    PageRange::new(
                        GuestAddress(start as u32),
                        ((end - start) / u64::from(crate::memory::PAGE_SIZE)) as u32,
                    )
                    .map_err(HostError::Memory)?,
                );
            }
            let candidate = self
                .windows_memory
                .prepare_initial_heap(self.memory.as_mut().unwrap(), &reservations)
                .map_err(HostError::WindowsMemory)?;
            plan.stage_write(
                GuestAddress(startup.peb + 0x18),
                &candidate
                    .default_heap()
                    .ok_or(HostError::Infrastructure)?
                    .to_le_bytes(),
            )
            .map_err(HostError::Module)?;
            Some(candidate)
        } else {
            None
        };
        let prepared_primary = if initial {
            Some(
                self.threads
                    .prepare_primary_thread(
                        thread.teb(),
                        PageRange::new(
                            GuestAddress(thread.config.stack_base),
                            thread.config.stack_pages,
                        )
                        .map_err(HostError::Memory)?,
                    )
                    .map_err(HostError::Thread)?,
            )
        } else {
            None
        };
        let saved = SavedModuleContext::capture(&ready_arena)?;
        let primary_created = if prepared_primary.is_some() {
            Some(self.process_creation_filetime()?)
        } else {
            None
        };
        let (new_tls, mut writes) = thread
            .stage_modules(&mut plan, self.memory()?, initial)
            .map_err(HostError::Module)?;
        let secondary_tls =
            self.prepare_secondary_static_tls(&mut plan, &new_tls, thread.allocation_bounds())?;
        writes
            .try_reserve_exact(secondary_tls.words().len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        writes.extend_from_slice(secondary_tls.words());
        let loader_change =
            self.prepare_process_loader_change(plan.additions(), plan.initialization_order(), &[])?;
        if let Some(change) = &loader_change {
            writes
                .try_reserve_exact(change.words().len())
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            writes.extend_from_slice(change.words());
        }
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
        let activation = self.stage_module_activation(&mut plan, activation_work)?;
        let loaded = plan
            .install_with_writes(
                self.memory.as_mut().ok_or(HostError::Closed)?,
                &mut self.modules.loaded,
                &writes,
            )
            .map_err(HostError::Module)?;
        self.commit_module_activation(activation);
        self.commit_secondary_static_tls(secondary_tls);
        self.commit_process_loader_change(loader_change);
        if let Some(startup) = prepared_startup {
            self.process_runtime.startup = Some(startup);
        }
        if let Some(memory) = prepared_memory {
            self.windows_memory = memory;
        }
        if let Some(primary) = prepared_primary {
            self.threads.commit_primary_thread(primary);
            self.threads
                .record_primary_creation(primary_created.unwrap());
        }
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
            let selector = initial_before.as_ref().unwrap().fs_selector;
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
            execution: self.park_module_execution(),
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
                saved,
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
