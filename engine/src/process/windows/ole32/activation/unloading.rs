use super::{
    ActivationRuntime32, Ole32CallbackOutcome32 as O, PendingActivation32, Phase32, ServerUse32,
    delivery::registered_callback,
};
use crate::{
    loader::ImportSymbol32,
    process::{
        EngineInstance, HostError, ModuleOperation32, calls::call::PendingOwner,
        windows::callbacks::GuestCallbackScratch32,
    },
    windows::{CallFrame32, com::ServerThreadingModel32, ole32::Ole32Api32},
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn begin_registered_com_unload(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: Ole32Api32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(false);
        };
        if let Some(slot) = runtime
            .activation
            .pending
            .iter()
            .position(|entry| entry.owner == owner && entry.token == token)
        {
            self.advance_registered_com_loading(slot)?;
            return Ok(true);
        }
        let thread = self.current_windows_thread_id()?;
        if api == Ole32Api32::CoUninitialize && runtime.apartments.initialization_count(thread) != 1
        {
            return Ok(false);
        }
        if runtime.apartments.check(thread).is_err() || runtime.activation.servers.is_empty() {
            return Ok(false);
        };
        if api == Ole32Api32::CoFreeUnusedLibrariesEx && frame.arguments()[1] != 0 {
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0))?);
            return Ok(true);
        }
        self.check_windows_callback_capacity()?;
        let delay = if api == Ole32Api32::CoFreeUnusedLibrariesEx {
            frame.arguments()[0]
        } else if api == Ole32Api32::CoUninitialize {
            0
        } else {
            u32::MAX
        };
        let continuation = self
            .ole32
            .as_mut()
            .unwrap()
            .activation
            .prepare()
            .map_err(|_| HostError::Infrastructure)?;
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            ActivationRuntime32::scratch_reservation(),
            &[0],
        )
        .map_err(HostError::Memory)?;
        let entry = PendingActivation32 {
            continuation,
            thread,
            owner,
            token,
            frame,
            api,
            out: 0,
            outer: 0,
            path: String::new(),
            scratch,
            phase: Phase32::UnloadNext { after: 0, delay },
            threading: ServerThreadingModel32::Both,
            server_threads: Vec::new(),
            server: None,
        };
        let runtime = &mut self.ole32.as_mut().unwrap().activation;
        runtime.next = continuation;
        runtime.pending.push(entry);
        let slot = runtime.pending.len() - 1;
        self.advance_registered_com_loading(slot)?;
        Ok(true)
    }

    pub(super) fn advance_registered_com_unload(
        &mut self,
        slot: usize,
        after: u64,
        delay: u32,
    ) -> Result<O, HostError> {
        self.ole32.as_mut().unwrap().activation.pending[slot].phase =
            Phase32::UnloadNext { after, delay };
        let ole32 = self.ole32.as_ref().unwrap();
        let runtime = &ole32.activation;
        let thread = runtime.pending[slot].thread;
        let final_uninitialize = runtime.pending[slot].api == Ole32Api32::CoUninitialize;
        if self
            .loader_initialization_owner()
            .is_some_and(|owner| owner != thread)
        {
            return Ok(O::LoaderPending);
        }
        let server = runtime
            .servers
            .iter()
            .filter(|server| server.id > after)
            .filter(|server| !final_uninitialize || server.closes_with(&ole32.apartments, thread))
            .filter(|server| {
                !matches!(
                    server.threading,
                    ServerThreadingModel32::Apartment | ServerThreadingModel32::MainSta
                ) || server.threads.contains(&thread)
            })
            .filter(|server| {
                !runtime.server_held(server.handle, slot)
                    && !runtime.unload_reserved(server.handle, slot)
            })
            .min_by_key(|server| server.id);
        let Some(server) = server else {
            return self
                .finish_registered_com_activation(slot, 0)
                .map(Into::into);
        };
        let lease = server.lease();
        let deadline = server.unused_deadline;
        let now = self.cpu_clock.monotonic_bounds().map_or(0, |(_, now)| now);
        if !final_uninitialize && deadline.is_some_and(|deadline| deadline > now) {
            return self.advance_registered_com_unload(slot, lease.server, delay);
        }
        let phase = if final_uninitialize || deadline.is_some() {
            Phase32::Unloading { lease, delay }
        } else {
            Phase32::ResolveUnload { lease, delay }
        };
        self.ole32.as_mut().unwrap().activation.pending[slot].phase = phase;
        let result = if matches!(phase, Phase32::Unloading { .. }) {
            self.free_library_module(lease.handle)
        } else {
            self.resolve_module_export(
                lease.handle,
                &ImportSymbol32::Name("DllCanUnloadNow".into()),
            )
        };
        self.finish_registered_com_unload_module(slot, result)
    }

    pub(super) fn accept_registered_com_unload_module(
        &mut self,
        slot: usize,
        result: Result<ModuleOperation32, HostError>,
    ) -> Result<(), HostError> {
        let outcome = self.finish_registered_com_unload_module(slot, result)?;
        self.accept_ole32_loader_outcome(outcome)
    }
    pub(super) fn finish_registered_com_unload_module(
        &mut self,
        slot: usize,
        result: Result<ModuleOperation32, HostError>,
    ) -> Result<O, HostError> {
        let entry = &self.ole32.as_ref().unwrap().activation.pending[slot];
        let (lease, delay) = match entry.phase {
            Phase32::ResolveUnload { lease, delay } | Phase32::Unloading { lease, delay } => {
                (lease, delay)
            }
            _ => return Err(HostError::Infrastructure),
        };
        match result {
            Ok(ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. }) => {
                Ok(O::LoaderPending)
            }
            Ok(ModuleOperation32::ExportResolved { address })
                if matches!(entry.phase, Phase32::ResolveUnload { .. }) =>
            {
                let request = registered_callback(entry.continuation, address, &[]);
                self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                    Phase32::CanUnload { lease, delay };
                Ok(O::Callback(request))
            }
            Ok(ModuleOperation32::Released) if matches!(entry.phase, Phase32::Unloading { .. }) => {
                self.ole32
                    .as_mut()
                    .unwrap()
                    .activation
                    .release_server(lease);
                self.advance_registered_com_unload(slot, lease.server, delay)
            }
            Ok(ModuleOperation32::InitializationFailed) => {
                self.advance_registered_com_unload(slot, lease.server, delay)
            }
            Err(error) => {
                use crate::{files::FileError32 as F, loader::ModuleError32 as M};
                let retryable = matches!(
                    error,
                    HostError::Module(
                        M::Capacity
                            | M::Allocation
                            | M::ReferenceExhausted
                            | M::Memory(_)
                            | M::Busy
                    ) | HostError::File(F::Capacity | F::Allocation | F::Exhausted)
                        | HostError::Activation(
                            crate::windows::activation::ActivationError32::NotEnoughMemory
                        )
                        | HostError::Memory(_)
                        | HostError::Infrastructure
                        | HostError::Call(_)
                );
                if retryable || super::loading::com_source_hresult(error).is_none() {
                    self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                        Phase32::UnloadNext {
                            after: lease.server.saturating_sub(1),
                            delay,
                        };
                    Err(error)
                } else {
                    self.advance_registered_com_unload(slot, lease.server, delay)
                }
            }
            _ => Err(HostError::Infrastructure),
        }
    }

    pub(super) fn resume_registered_com_unload(
        &mut self,
        slot: usize,
        lease: ServerUse32,
        delay: u32,
        result: u32,
    ) -> Result<O, HostError> {
        let Some(server) = self
            .ole32
            .as_mut()
            .unwrap()
            .activation
            .servers
            .iter_mut()
            .find(|server| server.id == lease.server && server.handle == lease.handle)
        else {
            return self.advance_registered_com_unload(slot, lease.server, delay);
        };
        if result != 0 || server.activity != lease.activity {
            server.unused_deadline = None;
            return self.advance_registered_com_unload(slot, lease.server, delay);
        };
        let effective_delay = if matches!(
            server.threading,
            ServerThreadingModel32::Apartment | ServerThreadingModel32::MainSta
        ) {
            0
        } else if delay == u32::MAX {
            600_000
        } else {
            delay
        };
        let now = self.cpu_clock.monotonic_bounds().map_or(0, |(_, now)| now);
        server.unused_deadline = Some(
            now.checked_add(u64::from(effective_delay) * 1_000_000)
                .ok_or(HostError::InvalidRequest)?,
        );
        if effective_delay != 0 {
            return self.advance_registered_com_unload(slot, lease.server, delay);
        };
        self.ole32.as_mut().unwrap().activation.pending[slot].phase =
            Phase32::Unloading { lease, delay };
        let result = self.free_library_module(lease.handle);
        let result = match result {
            Err(error) if !matches!(error, HostError::Module(_)) => {
                let runtime = &mut self.ole32.as_mut().unwrap().activation;
                runtime.pending[slot].phase = Phase32::CanUnload { lease, delay };
                if let Some(server) = runtime
                    .servers
                    .iter_mut()
                    .find(|server| server.id == lease.server)
                {
                    server.unused_deadline = None;
                }
                return Err(error);
            }
            result => result,
        };
        self.finish_registered_com_unload_module(slot, result)
    }
}
