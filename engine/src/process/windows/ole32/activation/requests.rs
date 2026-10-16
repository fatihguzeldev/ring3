use super::{ActivationRuntime32, PendingActivation32, Phase32};
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::memory::{failure, iid, output},
        },
    },
    windows::{
        CallFrame32,
        com::ServerThreadingModel32,
        ole32::{HResult32, IID_ICLASSFACTORY, KnownClass32, Ole32Api32},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn try_begin_registered_com_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: Ole32Api32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        if matches!(
            api,
            Ole32Api32::CoFreeUnusedLibraries
                | Ole32Api32::CoFreeUnusedLibrariesEx
                | Ole32Api32::CoUninitialize
        ) {
            return self.begin_registered_com_unload(owner, token, api, frame);
        }
        if !matches!(
            api,
            Ole32Api32::CoGetClassObject | Ole32Api32::CoCreateInstance
        ) {
            return Ok(false);
        }
        if let Some(slot) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .activation
                .pending
                .iter()
                .position(|pending| pending.owner == owner && pending.token == token)
        }) {
            self.advance_registered_com_loading(slot)?;
            return Ok(true);
        }
        let args = frame.arguments();
        let (class, requested) = match (
            iid(self.memory()?, args[0]),
            iid(self.memory()?, args[3]),
            output(self.memory()?, args[4]),
        ) {
            (Ok(class), Ok(requested), Ok(())) => (class, requested),
            _ => {
                self.publish_prepared_call(Self::prepare_completed_call(
                    frame.complete(HResult32::POINTER.0),
                )?);
                return Ok(true);
            }
        };
        let thread = self.current_windows_thread_id()?;
        let validation = (|| {
            let runtime = self.ole32.as_ref().ok_or(HResult32::NOT_INITIALIZED)?;
            runtime.apartments.check(thread)?;
            if api == Ole32Api32::CoGetClassObject && args[2] != 0 {
                return Err(HResult32::INVALID_ARGUMENT);
            }
            let flags = if api == Ole32Api32::CoCreateInstance {
                args[2]
            } else {
                args[1]
            };
            if flags & 1 == 0 {
                return Err(HResult32::CLASS_NOT_REGISTERED);
            }
            Ok(())
        })();
        if let Err(error) = validation {
            self.complete_registered_com_failure(frame, error)?;
            return Ok(true);
        }
        let resolution = match self.active_com_redirection(class.0)? {
            Some(server) => Ok(Some(crate::windows::com::InProcessServer32 {
                path: String::from_utf16(&server.server).map_err(|_| HostError::InvalidRequest)?,
                threading: server.threading,
            })),
            None => self.registered_com_server(class),
        };
        let mut server = match resolution {
            Ok(Some(server)) => server,
            Ok(None) if KnownClass32::from_id(class).is_some() => return Ok(false),
            Ok(None) => {
                self.complete_registered_com_failure(frame, HResult32::CLASS_NOT_REGISTERED)?;
                return Ok(true);
            }
            Err(error) => {
                self.complete_registered_com_failure(frame, error)?;
                return Ok(true);
            }
        };
        server.path = self.resolve_module_source_name(&server.path)?;
        let apartments = &self.ole32.as_ref().unwrap().apartments;
        let sta = apartments
            .is_single_threaded(thread)
            .map_err(|_| HostError::Infrastructure)?;
        let direct = match server.threading {
            ServerThreadingModel32::Both => true,
            ServerThreadingModel32::Neutral => false,
            ServerThreadingModel32::Apartment => sta,
            ServerThreadingModel32::Free => !sta,
            ServerThreadingModel32::MainSta => sta && apartments.main_sta_thread() == Some(thread),
        };
        if !direct {
            self.complete_registered_com_failure(frame, HResult32(0x8001_010e))?;
            return Ok(true);
        }
        let continuation = match self.ole32.as_mut().unwrap().activation.prepare() {
            Ok(id) => id,
            Err(error) => {
                self.complete_registered_com_failure(frame, error)?;
                return Ok(true);
            }
        };
        self.check_windows_callback_capacity()?;
        let mut server_threads = Vec::new();
        server_threads
            .try_reserve_exact(1)
            .map_err(|_| HostError::Infrastructure)?;
        server_threads.push(thread);
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let mut bytes = [0u8; 64];
        bytes[..16].copy_from_slice(&class.0);
        bytes[16..32].copy_from_slice(
            &if api == Ole32Api32::CoCreateInstance {
                IID_ICLASSFACTORY
            } else {
                requested
            }
            .0,
        );
        bytes[32..48].copy_from_slice(&requested.0);
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            ActivationRuntime32::scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(args[4]), &0u32.to_le_bytes())
            .map_err(HostError::Memory)?;
        let pending = PendingActivation32 {
            continuation,
            thread,
            owner,
            token,
            frame,
            api,
            out: args[4],
            outer: if api == Ole32Api32::CoCreateInstance {
                args[1]
            } else {
                0
            },
            path: server.path,
            scratch,
            phase: Phase32::Load,
            threading: server.threading,
            server_threads,
            server: None,
        };
        let runtime = &mut self.ole32.as_mut().unwrap().activation;
        runtime.next = continuation;
        runtime.pending.push(pending);
        self.advance_registered_com_loading(
            self.ole32.as_ref().unwrap().activation.pending.len() - 1,
        )?;
        Ok(true)
    }
    fn complete_registered_com_failure(
        &mut self,
        frame: CallFrame32,
        error: HResult32,
    ) -> Result<(), HostError> {
        self.memory()?
            .ensure_transaction_versions(1)
            .map_err(HostError::Memory)?;
        let result = failure(
            self.memory.as_mut().ok_or(HostError::Closed)?,
            frame.arguments()[4],
            error,
        )
        .unwrap_or(HResult32::POINTER.0);
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
        Ok(())
    }
}
