use super::{Ole32Config32, Ole32Error32};
use crate::{
    cpu::dbt::GateSpec,
    memory::GuestAddress,
    process::windows::objects::GuestComObjects32,
    windows::ole32::{Apartments32, FactoryMethod32, HResult32, KnownClass32},
};
pub(in crate::process) struct Ole32Runtime32 {
    pub(super) com: GuestComObjects32,
    pub(super) apartments: Apartments32,
    pub(super) factories: Vec<Option<KnownClass32>>,
    pub(super) methods: [GuestAddress; 5],
    pub(super) server_locks: [u32; 7],
}
impl Ole32Runtime32 {
    pub(in crate::process) fn new(config: Ole32Config32) -> Result<Self, Ole32Error32> {
        if config.max_objects == 0
            || config.max_objects > 65536
            || config.max_threads == 0
            || config.max_threads > 65536
        {
            return Err(Ole32Error32::InvalidConfiguration);
        }
        let com = GuestComObjects32::new(
            config.objects,
            config.max_objects as usize,
            config.max_objects as usize,
            1,
        )
        .map_err(|_| Ole32Error32::InvalidConfiguration)?;
        let apartments =
            Apartments32::new(config.max_threads as usize).map_err(|_| Ole32Error32::Allocation)?;
        let mut factories = Vec::new();
        factories
            .try_reserve_exact(config.max_objects as usize)
            .map_err(|_| Ole32Error32::Allocation)?;
        factories.resize_with(config.max_objects as usize, || None);
        Ok(Self {
            com,
            apartments,
            factories,
            methods: [GuestAddress(0); 5],
            server_locks: [0; 7],
        })
    }
    pub(super) fn initialize_methods(&mut self, gates: &[GateSpec]) -> Result<(), HResult32> {
        let mut methods = [GuestAddress(0); 5];
        for (slot, method) in methods.iter_mut().enumerate() {
            *method = gates
                .iter()
                .find(|gate| gate.id == FactoryMethod32(slot as u8).id())
                .ok_or(HResult32::UNEXPECTED)?
                .entry;
        }
        self.methods = methods;
        Ok(())
    }
}

impl Ole32Runtime32 {
    pub(in crate::process) fn guest_reservation(&self) -> crate::memory::PageRange {
        self.com.reservation()
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn check_com_thread_initialized(
        &self,
        thread: u32,
    ) -> Result<(), HResult32> {
        self.ole32
            .as_ref()
            .ok_or(HResult32::NOT_INITIALIZED)?
            .apartments
            .check(thread)
    }
}
