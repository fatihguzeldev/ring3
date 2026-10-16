use super::{
    Ole32Config32, Ole32Error32, Ole32Runtime32,
    memory::{com_error, failure, iid, output},
};
use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        ole32::{FactoryMethod32, HResult32, KnownClass32, Ole32Api32},
    },
};
impl EngineInstance {
    pub fn configure_ole32(&mut self, config: Ole32Config32) -> Result<(), HostError> {
        self.memory()?;
        if self.ole32.is_some() {
            return Err(HostError::Ole32(Ole32Error32::AlreadyConfigured));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.check_guest_reservation_admission(&[config.objects])?;
        self.ole32 = Some(Ole32Runtime32::new(config).map_err(HostError::Ole32)?);
        Ok(())
    }

    pub fn register_ole32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::ole32::ole32_module(base))
    }
    pub(in crate::process) fn try_complete_ole32_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let api = Ole32Api32::from_id(id);
        let method = FactoryMethod32::from_id(id);
        if api.is_none() && method.is_none() {
            return Ok(false);
        }
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        let count = api.map_or_else(|| method.unwrap().stack_words(), Ole32Api32::stack_words);
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != count {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        if let Some(api) = api.filter(|api| api.is_task_memory()) {
            let result = self.ole32_task_memory_call(api, frame.arguments())?;
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
            return Ok(true);
        }
        let thread = self.current_windows_thread_id()?;
        let gates = if api == Some(Ole32Api32::CoGetClassObject) {
            Some(self.module_gates()?)
        } else {
            None
        };
        if let Some(gates) = gates {
            self.ole32
                .as_mut()
                .ok_or(HostError::Ole32(Ole32Error32::NotConfigured))?
                .initialize_methods(&gates)
                .map_err(|_| HostError::Infrastructure)?;
        }
        let result = self
            .ole32_call(thread, api, method, frame.arguments())
            .unwrap_or_else(|error| error.0);
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
        Ok(true)
    }
    fn ole32_call(
        &mut self,
        thread: u32,
        api: Option<Ole32Api32>,
        method: Option<FactoryMethod32>,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        let runtime = self.ole32.as_mut().ok_or(HResult32::NOT_INITIALIZED)?;
        if let Some(api) = api {
            match api {
                Ole32Api32::CoInitialize => {
                    return runtime
                        .apartments
                        .initialize(thread, args[0], 2)
                        .map(|result| result.0);
                }
                Ole32Api32::CoInitializeEx => {
                    return runtime
                        .apartments
                        .initialize(thread, args[0], args[1])
                        .map(|result| result.0);
                }
                Ole32Api32::CoUninitialize => {
                    runtime.apartments.uninitialize(thread);
                    return Ok(0);
                }
                Ole32Api32::CoFreeUnusedLibraries | Ole32Api32::CoFreeUnusedLibrariesEx => {
                    runtime.apartments.check(thread)?;
                    return Ok(0);
                }
                Ole32Api32::CoGetClassObject | Ole32Api32::CoCreateInstance => {}
                Ole32Api32::CoTaskMemAlloc
                | Ole32Api32::CoTaskMemRealloc
                | Ole32Api32::CoTaskMemFree => unreachable!(),
            }
            let memory = self.memory.as_mut().ok_or(HResult32::UNEXPECTED)?;
            let out = args[4];
            output(memory, out)?;
            let class = iid(memory, args[0])?;
            let requested = iid(memory, args[3])?;
            if let Err(error) = runtime.apartments.check(thread) {
                return failure(memory, out, error);
            }
            let (class_context, outer) = if api == Ole32Api32::CoCreateInstance {
                (args[2], args[1])
            } else {
                (args[1], 0)
            };
            if api == Ole32Api32::CoGetClassObject && args[2] != 0 {
                return failure(memory, out, HResult32::INVALID_ARGUMENT);
            }
            let Some(class) = KnownClass32::from_id(class) else {
                return failure(memory, out, HResult32::CLASS_NOT_REGISTERED);
            };
            if class_context & 1 == 0 {
                return failure(memory, out, HResult32::CLASS_NOT_REGISTERED);
            }
            if outer != 0 {
                return failure(memory, out, HResult32::NO_AGGREGATION);
            }
            if api == Ole32Api32::CoGetClassObject {
                return runtime.publish_factory(memory, class, requested, out);
            }
            return self.activate_known_com_class(class, requested, out);
        }
        let method = method.unwrap();
        let pointer = args[0];
        let memory = self.memory.as_mut().ok_or(HResult32::UNEXPECTED)?;
        let class = runtime.factory_class(memory, pointer)?;
        match method.0 {
            0 => {
                let requested = iid(memory, args[1])?;
                runtime.query_factory(memory, pointer, requested, args[2])
            }
            1 => runtime
                .com
                .add_ref(memory, crate::memory::GuestAddress(pointer))
                .map_err(com_error),
            2 => runtime.release_factory(memory, pointer),
            3 => {
                let requested = iid(memory, args[2])?;
                output(memory, args[3])?;
                if args[1] != 0 {
                    return failure(memory, args[3], HResult32::NO_AGGREGATION);
                }
                self.activate_known_com_class(class, requested, args[3])
            }
            4 => runtime.lock_server(class, args[1] != 0),
            _ => Err(HResult32::INVALID_ARGUMENT),
        }
    }
}
