use super::{
    ExecutionApartment32,
    context::{PROXY_PAYLOAD, Proxy32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{
        EngineInstance, HostError,
        windows::{
            objects::{GuestComCreation32, GuestComError32, InterfaceLayout32},
            ole32::Ole32Runtime32,
        },
    },
    windows::{
        com::{IID_IUNKNOWN, InterfaceId},
        ole32::{ApartmentId32, HResult32, IID_ICLASSFACTORY},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn publish_standard_com_proxy(
        &mut self,
        execution: ExecutionApartment32,
        client: ApartmentId32,
        server: Option<u64>,
        pointer: u32,
        requested: InterfaceId,
        out: u32,
    ) -> Result<(), HostError> {
        if pointer == 0 || !matches!(requested, IID_IUNKNOWN | IID_ICLASSFACTORY) {
            return Err(HostError::InvalidRequest);
        }
        let runtime = self.ole32.as_mut().ok_or(HostError::InvalidRequest)?;
        let index = runtime
            .marshaling
            .proxies
            .iter()
            .position(Option::is_none)
            .ok_or(HostError::Infrastructure)?;
        let methods = runtime.methods;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let identity = runtime
            .com
            .create_with_lifetime(
                memory,
                PROXY_PAYLOAD | index as u64,
                &[
                    InterfaceLayout32 {
                        iid: IID_IUNKNOWN,
                        methods: &methods[..3],
                    },
                    InterfaceLayout32 {
                        iid: IID_ICLASSFACTORY,
                        methods: &methods,
                    },
                ],
                GuestComCreation32 {
                    output: Some((GuestAddress(out), requested)),
                    ..Default::default()
                },
            )
            .map_err(proxy_memory_error)?;
        let factory_view = runtime
            .com
            .query_target(memory, identity.identity_pointer, IID_ICLASSFACTORY)
            .expect("both standard proxy interfaces were published atomically");
        runtime.marshaling.proxies[index] = Some(Proxy32 {
            client,
            execution,
            object: pointer,
            factory: (requested == IID_ICLASSFACTORY).then_some(pointer),
            factory_extra: false,
            server,
            pointer: identity.identity_pointer,
            unknown: identity.identity_pointer,
            factory_view,
            disconnected: false,
            owned: [pointer, 0],
            local_alive: true,
            rundown: None,
        });
        Ok(())
    }
}

impl Ole32Runtime32 {
    pub(in crate::process::windows::ole32) fn proxy_index(
        &self,
        memory: &AddressSpace,
        pointer: u32,
    ) -> Result<Option<usize>, HostError> {
        let Ok(identity) = self.com.lookup(GuestAddress(pointer)) else {
            return Ok(None);
        };
        if identity.payload & PROXY_PAYLOAD == 0 {
            return Ok(None);
        }
        self.com
            .preflight(memory, GuestAddress(pointer))
            .map_err(proxy_memory_error)?;
        let index = (identity.payload & !PROXY_PAYLOAD) as usize;
        self.marshaling
            .proxies
            .get(index)
            .and_then(Option::as_ref)
            .ok_or(HostError::InvalidRequest)?;
        Ok(Some(index))
    }
    pub(in crate::process::windows::ole32) fn check_proxy_client(
        &self,
        index: usize,
        thread: u32,
    ) -> Result<(), HResult32> {
        let proxy = self.marshaling.proxies[index]
            .as_ref()
            .ok_or(HResult32::UNEXPECTED)?;
        if self.apartments.identity(thread)? != proxy.client {
            return Err(HResult32(0x8001_010e));
        }
        Ok(())
    }
}

pub(super) fn proxy_memory_error(error: GuestComError32) -> HostError {
    match error {
        GuestComError32::Memory(error) => HostError::Memory(error),
        _ => HostError::Infrastructure,
    }
}
