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
    pub(super) activation: super::activation::ActivationRuntime32,
    pub(super) streams: super::stream::StreamRuntime32,
    pub(super) marshaling: super::marshaling::MarshalingRuntime32,
}
impl Ole32Runtime32 {
    pub(in crate::process) fn com_host_origin(&self, thread: u32, sta: bool) -> Option<u32> {
        (sta && self.apartments.identity(thread)
            == Ok(crate::windows::ole32::ApartmentId32::Neutral)
            && self.apartments.is_single_threaded(thread) == Ok(true))
        .then_some(thread)
    }
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
            config.max_objects as usize * 3,
            7,
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
            activation: Default::default(),
            streams: super::stream::StreamRuntime32::new(config.max_objects as usize)?,
            marshaling: super::marshaling::MarshalingRuntime32::new(config.max_objects as usize)?,
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

impl Ole32Runtime32 {
    pub(in crate::process) fn check_host_apartment(
        &self,
        thread: u32,
        sta: bool,
    ) -> Result<(), crate::process::HostError> {
        self.apartments
            .check_initialize(thread, 0, if sta { 2 } else { 0 })
            .map_err(|_| crate::process::HostError::Infrastructure)
    }
    pub(in crate::process) fn commit_host_apartment(&mut self, thread: u32, sta: bool) {
        self.apartments
            .initialize(thread, 0, if sta { 2 } else { 0 })
            .expect("host apartment capacity was preflighted before canonical thread publication");
        self.apartments.mark_host(thread);
    }
    pub(in crate::process) fn guest_reservation(&self) -> crate::memory::PageRange {
        self.com.reservation()
    }

    pub(in crate::process) fn activation_scratch_reservation(&self) -> crate::memory::PageRange {
        super::activation::ActivationRuntime32::scratch_reservation()
    }
    pub(in crate::process) fn commit_thread_exit(&mut self, thread: u32) {
        self.apartments.terminate_thread(thread);
    }
}
