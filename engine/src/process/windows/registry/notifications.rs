use super::{RegistryError32 as E, RegistryReply32, RegistryRuntime32};
use crate::{
    process::{EngineInstance, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        registry::{PreparedRegistry32, RegistryWatch32},
        synchronization::{EventBinding32, PreparedIoSignals32, SynchronizationContext32},
    },
};
#[derive(Clone, Copy)]
pub(super) struct Registration32 {
    pub id: u64,
    pub handle: u32,
    pub thread: u32,
    pub agnostic: bool,
    pub watch: RegistryWatch32,
    pub event: Option<EventBinding32>,
    pub ready: bool,
}
pub(super) struct NotificationPlan32 {
    ids: Vec<u64>,
    signals: Option<PreparedIoSignals32>,
}
impl RegistryRuntime32 {
    fn notification_plan(
        &self,
        sync: Option<&SynchronizationContext32>,
        matches: impl Fn(&Registration32) -> bool,
    ) -> Result<NotificationPlan32, E> {
        let mut ids = Vec::new();
        let mut bindings = Vec::new();
        for registration in self
            .watches
            .iter()
            .flatten()
            .filter(|registration| !registration.ready && matches(registration))
        {
            ids.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            ids.push(registration.id);
            if let Some(binding) = registration.event {
                bindings.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                bindings.push(binding);
            }
        }
        let signals = if bindings.is_empty() {
            None
        } else {
            Some(
                sync.ok_or(E::CallNotImplemented)?
                    .prepare_signals_and_release(&bindings)
                    .map_err(sync_error)?,
            )
        };
        Ok(NotificationPlan32 { ids, signals })
    }
    pub(super) fn prepare_notifications(
        &self,
        prepared: &PreparedRegistry32,
        sync: Option<&SynchronizationContext32>,
    ) -> Result<NotificationPlan32, E> {
        self.notification_plan(sync, |registration| {
            self.native.notifies(prepared, registration.watch)
        })
    }
    pub(super) fn prepare_close_notifications(
        &self,
        handle: u32,
        sync: Option<&SynchronizationContext32>,
    ) -> Result<NotificationPlan32, E> {
        self.notification_plan(sync, |registration| {
            handle & 0x80000000 == 0 && registration.handle == handle
        })
    }
    pub(super) fn commit_notifications(
        &mut self,
        plan: NotificationPlan32,
        sync: Option<&mut SynchronizationContext32>,
    ) {
        for registration in &mut self.watches {
            if registration
                .as_ref()
                .is_some_and(|registration| plan.ids.contains(&registration.id))
            {
                if registration.as_ref().unwrap().event.is_some() {
                    *registration = None;
                } else {
                    registration.as_mut().unwrap().ready = true;
                }
            }
        }
        if let Some(signals) = plan.signals {
            sync.unwrap().commit_signals_and_release(signals);
        }
    }
    pub(super) fn watch_ready(&self, id: u64) -> bool {
        self.watches
            .iter()
            .flatten()
            .find(|registration| registration.id == id)
            .is_none_or(|registration| registration.ready)
    }
    pub(super) fn finish_watch(&mut self, id: u64) {
        for registration in &mut self.watches {
            if registration
                .as_ref()
                .is_some_and(|registration| registration.id == id)
            {
                *registration = None;
                break;
            }
        }
    }
    pub(in crate::process) fn thread_exit_bindings(
        &self,
        thread: u32,
    ) -> Result<Vec<EventBinding32>, E> {
        let mut bindings = Vec::new();
        for registration in self
            .watches
            .iter()
            .flatten()
            .filter(|registration| !registration.agnostic && registration.thread == thread)
        {
            if let Some(binding) = registration.event {
                bindings.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                bindings.push(binding);
            }
        }
        Ok(bindings)
    }
    pub(in crate::process) fn commit_thread_exit(&mut self, thread: u32) {
        for registration in &mut self.watches {
            if registration
                .as_ref()
                .is_some_and(|registration| !registration.agnostic && registration.thread == thread)
            {
                *registration = None;
            }
        }
        if self.reply.is_some_and(|reply| reply.thread == thread) {
            self.reply = None;
        }
    }
    pub(in crate::process) fn process_exit_bindings(&self) -> Result<Vec<EventBinding32>, E> {
        let mut bindings = Vec::new();
        for registration in self.watches.iter().flatten() {
            if let Some(binding) = registration.event {
                bindings.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                bindings.push(binding);
            }
        }
        Ok(bindings)
    }
    pub(in crate::process) fn commit_process_exit(&mut self) {
        self.watches.fill(None);
        self.reply = None;
    }
}
impl EngineInstance {
    pub(super) fn begin_registry_notify(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        thread: u32,
    ) -> Result<bool, E> {
        let args = frame.arguments();
        let runtime = self.registry.as_mut().ok_or(E::NotConfigured)?;
        if runtime.reply.is_some() {
            return Err(E::Busy);
        }
        let watch = runtime.native.watch(args[0], args[1] != 0, args[2])?;
        let slot = runtime
            .watches
            .iter()
            .position(Option::is_none)
            .ok_or(E::NotEnoughMemory)?;
        let id = runtime
            .next_watch
            .checked_add(1)
            .ok_or(E::NotEnoughMemory)?;
        let event = if args[4] != 0 {
            let sync = self
                .synchronization
                .as_mut()
                .ok_or(E::CallNotImplemented)?
                .context_mut();
            let prepared = sync
                .prepare_event_bindings(&[args[3]], &[])
                .map_err(sync_error)?;
            Some(sync.commit_event_bindings(prepared)[0])
        } else {
            None
        };
        runtime.watches[slot] = Some(Registration32 {
            id,
            handle: args[0],
            thread,
            agnostic: args[2] & 0x10000000 != 0,
            watch,
            event,
            ready: false,
        });
        runtime.next_watch = id;
        if args[4] == 0 {
            runtime.reply = Some(RegistryReply32 {
                owner,
                token,
                frame,
                thread,
                wait: super::context::RegistryWaitKind32::Notify(id),
            });
            Ok(false)
        } else {
            Ok(true)
        }
    }
}

fn sync_error(error: crate::windows::synchronization::SynchronizationError32) -> E {
    use crate::windows::synchronization::SynchronizationError32 as S;
    match error {
        S::InvalidHandle | S::Stale => E::InvalidHandle,
        S::AccessDenied => E::AccessDenied,
        S::Capacity | S::Allocation | S::Exhausted => E::NotEnoughMemory,
        S::Unsupported => E::CallNotImplemented,
        _ => E::InvalidParameter,
    }
}
