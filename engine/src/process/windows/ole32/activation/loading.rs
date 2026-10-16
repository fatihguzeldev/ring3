use super::{Phase32, Server32, delivery::registered_callback};
use crate::{
    loader::ImportSymbol32,
    process::{
        EngineInstance, HostError, ModuleOperation32, calls::call::PendingOwner,
        windows::callbacks::WindowsProviderOutcome32,
    },
    windows::ole32::{HResult32, Ole32Api32},
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn advance_registered_com_loading(
        &mut self,
        slot: usize,
    ) -> Result<(), HostError> {
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        if self
            .checked_pending_call(pending.owner, pending.token)?
            .frame
            != pending.frame
        {
            return Err(HostError::InvalidRequest);
        }
        pending
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        if self
            .loader_initialization_owner()
            .is_some_and(|owner| owner != pending.thread)
        {
            return Ok(());
        }
        match pending.phase {
            Phase32::Load => {
                if !self.ole32.as_ref().unwrap().activation.load_ready(slot) {
                    return Ok(());
                }
                let thread = pending.thread;
                let threading = pending.threading;
                let (mta_lease, neutral_lease) = shared_server_lease(
                    &self.ole32.as_ref().unwrap().apartments,
                    thread,
                    threading,
                )?;
                let existing = self
                    .ole32
                    .as_ref()
                    .unwrap()
                    .activation
                    .servers
                    .iter()
                    .position(|server| server.path.eq_ignore_ascii_case(&pending.path));
                if let Some(index) = existing {
                    let server = &mut self.ole32.as_mut().unwrap().activation.servers[index];
                    let activity = server
                        .activity
                        .checked_add(1)
                        .ok_or(HostError::Infrastructure)?;
                    if !server.threads.contains(&thread) {
                        server
                            .threads
                            .try_reserve(1)
                            .map_err(|_| HostError::Infrastructure)?;
                        server.threads.push(thread);
                    }
                    if matches!(
                        threading,
                        crate::windows::com::ServerThreadingModel32::Both
                            | crate::windows::com::ServerThreadingModel32::Free
                            | crate::windows::com::ServerThreadingModel32::Neutral
                    ) {
                        server.threading = threading;
                    }
                    server.mta_lease |= mta_lease;
                    server.neutral_lease |= neutral_lease;
                    server.unused_deadline = None;
                    server.activity = activity;
                    let handle = server.handle;
                    let id = server.id;
                    let pending = &mut self.ole32.as_mut().unwrap().activation.pending[slot];
                    pending.server = Some(id);
                    pending.phase = Phase32::Resolve { handle };
                    return self.advance_registered_com_loading(slot);
                }
                if self.ole32.as_ref().unwrap().activation.servers.len() >= 256 {
                    return self.accept_registered_com_module_result(
                        slot,
                        Err(HostError::Module(crate::loader::ModuleError32::Capacity)),
                    );
                }
                let path = core::mem::take(
                    &mut self.ole32.as_mut().unwrap().activation.pending[slot].path,
                );
                self.ole32.as_mut().unwrap().activation.pending[slot].phase = Phase32::Loading;
                let result = self.load_library_module(&path, None);
                self.ole32.as_mut().unwrap().activation.pending[slot].path = path;
                self.accept_registered_com_module_result(slot, result)
            }
            Phase32::Resolve { handle } => {
                self.ole32.as_mut().unwrap().activation.pending[slot].phase = Phase32::Resolving;
                let result = self.resolve_module_export(
                    handle,
                    &ImportSymbol32::Name("DllGetClassObject".into()),
                );
                self.accept_registered_com_module_result(slot, result)
            }
            Phase32::UnloadNext { after, delay } => {
                let outcome = self.advance_registered_com_unload(slot, after, delay)?;
                self.accept_ole32_loader_outcome(outcome)
            }
            Phase32::Finish { result } => {
                let frame = pending.frame;
                let WindowsProviderOutcome32::Return(result) =
                    self.finish_registered_com_activation(slot, result)?
                else {
                    unreachable!()
                };
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
                Ok(())
            }
            Phase32::Loading
            | Phase32::Resolving
            | Phase32::Factory
            | Phase32::Create { .. }
            | Phase32::Release { .. }
            | Phase32::ResolveUnload { .. }
            | Phase32::CanUnload { .. }
            | Phase32::Unloading { .. } => Ok(()),
        }
    }
    pub(super) fn accept_registered_com_module_result(
        &mut self,
        slot: usize,
        result: Result<ModuleOperation32, HostError>,
    ) -> Result<(), HostError> {
        self.ole32.as_ref().unwrap().activation.pending[slot]
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        if matches!(
            self.ole32.as_ref().unwrap().activation.pending[slot].phase,
            Phase32::ResolveUnload { .. } | Phase32::Unloading { .. }
        ) {
            return self.accept_registered_com_unload_module(slot, result);
        }
        let operation = match result {
            Ok(ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. }) => {
                return Ok(());
            }
            Ok(operation) => operation,
            Err(error) => {
                if let Some(result) = com_source_hresult(error) {
                    self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                        Phase32::Finish { result };
                    return self.advance_registered_com_loading(slot);
                }
                let runtime = &mut self.ole32.as_mut().unwrap().activation;
                let pending = &mut runtime.pending[slot];
                if pending.phase == Phase32::Loading {
                    pending.phase = Phase32::Load;
                } else if pending.phase == Phase32::Resolving {
                    let server = runtime
                        .servers
                        .iter()
                        .find(|server| Some(server.id) == pending.server)
                        .ok_or(HostError::Infrastructure)?;
                    pending.phase = Phase32::Resolve {
                        handle: server.handle,
                    };
                }
                return Err(error);
            }
        };
        match (
            self.ole32.as_ref().unwrap().activation.pending[slot].phase,
            operation,
        ) {
            (Phase32::Loading, ModuleOperation32::Loaded { handle }) => {
                let runtime = self.ole32.as_ref().unwrap();
                let pending = &runtime.activation.pending[slot];
                let (mta_lease, neutral_lease) =
                    shared_server_lease(&runtime.apartments, pending.thread, pending.threading)?;
                let pending = &mut self.ole32.as_mut().unwrap().activation.pending[slot];
                pending.server = Some(pending.continuation);
                let server = Server32 {
                    id: pending.continuation,
                    path: core::mem::take(&mut pending.path),
                    handle,
                    threading: pending.threading,
                    threads: core::mem::take(&mut pending.server_threads),
                    mta_lease,
                    neutral_lease,
                    unused_deadline: None,
                    activity: 1,
                };
                self.ole32.as_mut().unwrap().activation.servers.push(server);
                self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                    Phase32::Resolve { handle };
                self.advance_registered_com_loading(slot)
            }
            (Phase32::Resolving, ModuleOperation32::ExportResolved { address }) => {
                let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
                let out = if pending.api == Ole32Api32::CoCreateInstance {
                    pending.scratch.address.0 + 48
                } else {
                    pending.out
                };
                let request = registered_callback(
                    pending.continuation,
                    address,
                    &[
                        pending.scratch.address.0,
                        pending.scratch.address.0 + 16,
                        out,
                    ],
                );
                self.park_ole32_callback(request)?;
                self.ole32.as_mut().unwrap().activation.pending[slot].phase = Phase32::Factory;
                Ok(())
            }
            (_, ModuleOperation32::InitializationFailed) => self
                .accept_registered_com_module_result(
                    slot,
                    Err(HostError::Module(
                        crate::loader::ModuleError32::InitializationFailed,
                    )),
                ),
            _ => Err(HostError::Infrastructure),
        }
    }
    pub(in crate::process) fn complete_registered_com_module_operation(
        &mut self,
        operation: ModuleOperation32,
    ) -> Result<bool, HostError> {
        let Some(pending) = self.pending_call.as_ref() else {
            return Ok(false);
        };
        let Some(slot) = self.ole32.as_ref().and_then(|runtime| {
            runtime.activation.pending.iter().position(|entry| {
                entry.owner == pending.owner
                    && entry.token == pending.token
                    && matches!(
                        entry.phase,
                        Phase32::Loading
                            | Phase32::Resolving
                            | Phase32::ResolveUnload { .. }
                            | Phase32::Unloading { .. }
                            | Phase32::Resolve { .. }
                            | Phase32::Finish { .. }
                            | Phase32::UnloadNext { .. }
                    )
            })
        }) else {
            return Ok(false);
        };
        if matches!(
            self.ole32.as_ref().unwrap().activation.pending[slot].phase,
            Phase32::Resolve { .. } | Phase32::Finish { .. } | Phase32::UnloadNext { .. }
        ) {
            self.advance_registered_com_loading(slot)?;
        } else {
            self.accept_registered_com_module_result(slot, Ok(operation))?;
        }
        Ok(true)
    }
    pub(in crate::process) fn fail_registered_com_module_source(
        &mut self,
        error: HostError,
    ) -> Result<bool, HostError> {
        use crate::{files::FileError32 as F, loader::ModuleError32 as M};
        let Some(call) = self.pending_call.as_ref() else {
            return Ok(false);
        };
        let Some(slot) = self.ole32.as_ref().and_then(|runtime| {
            runtime.activation.pending.iter().position(|entry| {
                entry.owner == call.owner
                    && entry.token == call.token
                    && entry.thread == self.threads.current_id()
                    && matches!(
                        entry.phase,
                        Phase32::Loading
                            | Phase32::Resolving
                            | Phase32::ResolveUnload { .. }
                            | Phase32::Unloading { .. }
                    )
            })
        }) else {
            return Ok(false);
        };
        if matches!(
            self.ole32.as_ref().unwrap().activation.pending[slot].phase,
            Phase32::ResolveUnload { .. } | Phase32::Unloading { .. }
        ) {
            let outcome = self.finish_registered_com_unload_module(slot, Err(error))?;
            self.accept_ole32_loader_outcome(outcome)?;
            return Ok(true);
        }
        let result = match error {
            HostError::Module(
                M::Capacity | M::Allocation | M::ReferenceExhausted | M::Memory(_),
            )
            | HostError::Activation(
                crate::windows::activation::ActivationError32::NotEnoughMemory,
            )
            | HostError::File(F::Capacity | F::Allocation | F::Exhausted)
            | HostError::Memory(_)
            | HostError::Infrastructure => {
                let runtime = &mut self.ole32.as_mut().unwrap().activation;
                let entry = &mut runtime.pending[slot];
                entry.phase = if entry.phase == Phase32::Loading {
                    Phase32::Load
                } else {
                    let server = runtime
                        .servers
                        .iter()
                        .find(|server| Some(server.id) == entry.server)
                        .ok_or(HostError::Infrastructure)?;
                    Phase32::Resolve {
                        handle: server.handle,
                    }
                };
                return Err(error);
            }
            other => match com_source_hresult(other) {
                Some(result) => result,
                None => return Err(other),
            },
        };
        self.ole32.as_mut().unwrap().activation.pending[slot].phase = Phase32::Finish { result };
        self.advance_registered_com_loading(slot)?;
        Ok(true)
    }
    pub(in crate::process) fn registered_com_wait_ready(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .activation
                .pending
                .iter()
                .enumerate()
                .any(|(slot, entry)| {
                    entry.thread == thread
                        && match entry.phase {
                            Phase32::Load => runtime.activation.load_ready(slot),
                            Phase32::Resolve { .. }
                            | Phase32::UnloadNext { .. }
                            | Phase32::Finish { .. } => true,
                            _ => false,
                        }
                })
        }) && self
            .loader_initialization_owner()
            .is_none_or(|owner| owner == thread)
    }
    pub(in crate::process) fn resume_registered_com_loader_wait(
        &mut self,
    ) -> Result<(), HostError> {
        let thread = self.threads.current_id();
        if !self.registered_com_wait_ready(thread) {
            return Ok(());
        }
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        let slot = self
            .ole32
            .as_ref()
            .unwrap()
            .activation
            .pending
            .iter()
            .position(|entry| {
                entry.thread == thread
                    && entry.owner == pending.owner
                    && entry.token == pending.token
            })
            .ok_or(HostError::Infrastructure)?;
        self.advance_registered_com_loading(slot)
    }
    pub(in crate::process) fn ole32_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self.ole32.as_ref().is_some_and(|runtime| {
                runtime
                    .activation
                    .pending
                    .iter()
                    .any(|entry| entry.token == token)
            })
    }
    pub(in crate::process) fn registered_com_owns_call(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .activation
                .pending
                .iter()
                .any(|entry| entry.owner == owner && entry.token == token)
        })
    }
    pub(in crate::process) fn ole32_owns_loader_call(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.activation.pending.iter().any(|entry| {
                entry.owner == owner
                    && entry.token == token
                    && self.loader_initialization_owner() == Some(entry.thread)
                    && matches!(
                        entry.phase,
                        Phase32::Loading
                            | Phase32::Resolving
                            | Phase32::ResolveUnload { .. }
                            | Phase32::Unloading { .. }
                    )
            })
        })
    }
    pub(in crate::process) fn ole32_pending_owners(
        &self,
    ) -> impl Iterator<Item = PendingOwner> + '_ {
        self.ole32
            .iter()
            .flat_map(|runtime| runtime.activation.pending.iter().map(|entry| entry.owner))
    }
}

pub(super) fn com_source_hresult(error: HostError) -> Option<u32> {
    use crate::{files::FileError32 as F, loader::ModuleError32 as M};
    Some(match error {
        HostError::Module(M::MissingModule) | HostError::File(F::NotFound | F::PathNotFound) => {
            HResult32::DLL_NOT_FOUND.0
        }
        HostError::Module(M::Pe(_) | M::InitializationFailed | M::InvalidModuleKind) => {
            HResult32::ERROR_IN_DLL.0
        }
        HostError::Module(M::Capacity | M::Allocation | M::ReferenceExhausted) => {
            HResult32::OUT_OF_MEMORY.0
        }
        HostError::Module(M::Memory(_)) => return None,
        HostError::Module(_) => HResult32::CLASS_NOT_AVAILABLE.0,
        HostError::Activation(crate::windows::activation::ActivationError32::NotEnoughMemory)
        | HostError::File(F::Capacity | F::Allocation | F::Exhausted) => return None,
        HostError::Activation(error) => 0x8007_0000 | error.win32(),
        HostError::File(error) => 0x8007_0000 | error.win32(),
        _ => return None,
    })
}

fn shared_server_lease(
    apartments: &crate::windows::ole32::Apartments32,
    thread: u32,
    model: crate::windows::com::ServerThreadingModel32,
) -> Result<(bool, bool), HostError> {
    use crate::windows::com::ServerThreadingModel32 as M;
    Ok(match model {
        M::Free => (true, false),
        M::Neutral => (false, true),
        M::Both => (
            !apartments
                .is_single_threaded(thread)
                .map_err(|_| HostError::InvalidRequest)?,
            false,
        ),
        M::Apartment | M::MainSta => (false, false),
    })
}
