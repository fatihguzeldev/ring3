use super::{Ole32CallbackOutcome32, Phase32};
use crate::{
    memory::{Access, GuestAddress},
    process::{
        EngineInstance, HostError,
        windows::callbacks::{
            WindowsCallback32, WindowsCallbackProvider32, WindowsProviderOutcome32,
        },
    },
    windows::ole32::{HResult32, Ole32Api32},
};

impl EngineInstance {
    pub(in crate::process) fn check_registered_com_callback(
        &self,
        owner: crate::process::calls::call::PendingOwner,
        token: u32,
        frame: crate::windows::CallFrame32,
        continuation: u64,
    ) -> Result<(), HostError> {
        let thread = self.current_windows_thread_id()?;
        let pending = self
            .ole32
            .as_ref()
            .and_then(|runtime| {
                runtime.activation.pending.iter().find(|pending| {
                    pending.owner == owner
                        && pending.token == token
                        && pending.frame == frame
                        && pending.continuation == continuation
                        && pending.thread == thread
                        && matches!(
                            pending.phase,
                            Phase32::Factory
                                | Phase32::Create { .. }
                                | Phase32::Release { .. }
                                | Phase32::CanUnload { .. }
                        )
                })
            })
            .ok_or(HostError::InvalidRequest)?;
        pending
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)
    }
    pub(in crate::process) fn resume_ole32_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<Ole32CallbackOutcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let slot = self
            .ole32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .activation
            .pending
            .iter()
            .position(|pending| pending.continuation == continuation && pending.thread == thread)
            .ok_or(HostError::InvalidRequest)?;
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        pending
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        if let Phase32::CanUnload { lease, delay } = pending.phase {
            return self.resume_registered_com_unload(slot, lease, delay, result);
        }
        if let Phase32::UnloadNext { after, delay } = pending.phase {
            return self.advance_registered_com_unload(slot, after, delay);
        }
        let outcome = match pending.phase {
            Phase32::Factory
                if pending.api == Ole32Api32::CoGetClassObject || result & 0x8000_0000 != 0 =>
            {
                self.finish_registered_com_activation(slot, result)
            }
            Phase32::Factory => {
                let factory = self.ole32_read_word(pending.scratch.address.0 + 48)?;
                if factory == 0 {
                    return self
                        .finish_registered_com_activation(slot, HResult32::UNEXPECTED.0)
                        .map(Into::into);
                }
                let table = self.ole32_read_word(factory)?;
                let release =
                    self.ole32_read_word(table.checked_add(8).ok_or(HostError::InvalidRequest)?)?;
                let create =
                    self.ole32_read_word(table.checked_add(12).ok_or(HostError::InvalidRequest)?)?;
                self.memory()?
                    .check_span(GuestAddress(release), 1, Access::Execute)
                    .map_err(HostError::Memory)?;
                self.memory()?
                    .check_span(GuestAddress(create), 1, Access::Execute)
                    .map_err(HostError::Memory)?;
                let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
                let out = if self.remote_com_activation(continuation) {
                    pending.scratch.address.0 + 52
                } else {
                    pending.out
                };
                let request = registered_callback(
                    continuation,
                    create,
                    &[factory, pending.outer, pending.scratch.address.0 + 32, out],
                );
                self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                    Phase32::Create { factory, release };
                Ok(WindowsProviderOutcome32::Callback(request))
            }
            Phase32::Create { factory, release } => {
                let request = registered_callback(continuation, release, &[factory]);
                self.ole32.as_mut().unwrap().activation.pending[slot].phase =
                    Phase32::Release { result };
                Ok(WindowsProviderOutcome32::Callback(request))
            }
            Phase32::Release { result } => self.finish_registered_com_activation(slot, result),
            _ => Err(HostError::InvalidRequest),
        };
        outcome.map(Into::into)
    }
    pub(in crate::process::windows::ole32) fn finish_registered_com_activation(
        &mut self,
        slot: usize,
        mut result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        pending
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        if result & 0x8000_0000 == 0
            && let Some(remote) = self
                .ole32
                .as_ref()
                .unwrap()
                .marshaling
                .activations
                .iter()
                .find(|remote| remote.continuation == pending.continuation)
        {
            let execution = remote.execution;
            let client = remote.client;
            let server = pending.server;
            let out = pending.out;
            let pointer = self.ole32_read_word(pending.scratch.address.0 + 52)?;
            let requested =
                super::super::memory::iid(self.memory()?, pending.scratch.address.0 + 32)
                    .map_err(|_| HostError::InvalidRequest)?;
            if pointer == 0 {
                result = HResult32::UNEXPECTED.0;
            } else {
                self.publish_standard_com_proxy(
                    execution, client, server, pointer, requested, out,
                )?;
            }
        }
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        if result & 0x8000_0000 != 0 {
            self.memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(pending.out), &0u32.to_le_bytes())
                .map_err(HostError::Memory)?;
        }
        pending
            .scratch
            .release(self.memory.as_mut().unwrap())
            .map_err(HostError::Memory)?;
        if pending.api == Ole32Api32::CoUninitialize {
            let thread = pending.thread;
            let runtime = self.ole32.as_mut().unwrap();
            runtime.apartments.uninitialize(thread);
            for server in &mut runtime.activation.servers {
                server.threads.retain(|owner| *owner != thread);
            }
        }
        let continuation = self.ole32.as_ref().unwrap().activation.pending[slot].continuation;
        self.ole32
            .as_mut()
            .unwrap()
            .marshaling
            .activations
            .retain(|remote| remote.continuation != continuation);
        self.ole32
            .as_mut()
            .unwrap()
            .activation
            .pending
            .swap_remove(slot);
        Ok(WindowsProviderOutcome32::Return(result))
    }
    pub(in crate::process::windows::ole32) fn ole32_read_word(
        &self,
        address: u32,
    ) -> Result<u32, HostError> {
        let mut bytes = [0u8; 4];
        self.memory()?
            .read(GuestAddress(address), &mut bytes)
            .map_err(HostError::Memory)?;
        Ok(u32::from_le_bytes(bytes))
    }
}
pub(super) fn registered_callback(
    continuation: u64,
    procedure: u32,
    args: &[u32],
) -> WindowsCallback32 {
    let mut arguments = [0u32; 16];
    arguments[..args.len()].copy_from_slice(args);
    WindowsCallback32 {
        procedure: GuestAddress(procedure),
        arguments,
        count: args.len() as u32,
        continuation,
        provider: WindowsCallbackProvider32::Ole32,
    }
}
