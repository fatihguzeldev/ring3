mod calls;
mod chain;
mod context;
mod cpu;
mod delivery;
mod dispatch;
mod registration;
mod termination;

use crate::{
    process::{
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
    },
    windows::exceptions::{
        ExceptionRecord32, HandlerId32, HandlerRegistry32, context::ContextState32,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ExceptionPhase32 {
    Vectored = 1,
    Structured = 2,
    Filter = 3,
    Continue = 4,
    Unwind = 5,
    Unhandled = 6,
    Resumed = 7,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExceptionDispatch32 {
    pub id: u64,
    pub thread: u32,
    pub phase: ExceptionPhase32,
    pub procedure: u32,
    pub record: u32,
    pub context: u32,
    pub return_pc: u32,
    pub return_id: u32,
    pub code: u32,
}
pub(in crate::process) struct ExceptionRuntime32 {
    handlers: HandlerRegistry32,
    deliveries: Vec<Delivery32>,
    next: u64,
}
impl ExceptionRuntime32 {
    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn has_deliveries(&self) -> bool {
        !self.deliveries.is_empty()
    }

    pub(in crate::process) fn new() -> Result<Self, crate::windows::exceptions::ExceptionError32> {
        Ok(Self {
            handlers: HandlerRegistry32::new(1024)?,
            deliveries: Vec::new(),
            next: 0,
        })
    }
    pub(in crate::process) fn retire_thread(&mut self, thread: u32) {
        self.deliveries.retain(|delivery| delivery.thread != thread);
    }
    pub(in crate::process) fn deliveries_clear_process_exit(&mut self) {
        self.deliveries.clear();
    }
    pub(in crate::process) fn execution_pins(&self) -> impl Iterator<Item = u64> + '_ {
        self.deliveries
            .iter()
            .filter_map(|delivery| match delivery.owner {
                PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => {
                    Some(id)
                }
                _ => None,
            })
    }
}
struct Delivery32 {
    id: u64,
    thread: u32,
    owner: PendingOwner,
    record: ExceptionRecord32,
    original: ContextState32,
    extended: ExtendedCpuState,
    pending: Option<PendingCall>,
    event: Option<crate::process::CpuEvent32>,
    phase: ExceptionPhase32,
    handlers: Vec<HandlerId32>,
    cursor: usize,
    chain: u32,
    nested_boundary: Option<u32>,
    popped: Vec<registration::PoppedRegistration32>,
    visited: Vec<u32>,
    stack_low: u32,
    stack_high: u32,
    record_address: u32,
    context_address: u32,
    pointers_address: u32,
    dispatcher_address: u32,
    stack: u32,
    return_pc: u32,
    active: Option<ActiveHandler32>,
    unwind: Option<Unwind32>,
    filter_return: bool,
    chain_write: Option<u32>,
    record_output: Option<u32>,
    initial_page_change: Option<crate::memory::PrivatePageChange32>,
    initial_stack_limit: Option<crate::memory::WordWrite32>,
}
#[derive(Default)]
struct ExceptionInitial32 {
    filter: Option<crate::windows::exceptions::context::Win32Context32>,
    page_change: Option<crate::memory::PrivatePageChange32>,
    stack_overflow: Option<crate::process::windows::threads::PreparedStackOverflow32>,
    last_chance: bool,
}
#[derive(Clone, Copy)]
struct ActiveHandler32 {
    procedure: u32,
    return_esp: u32,
    establisher: u32,
    next: u32,
    registration: Option<registration::HandlerRegistration32>,
    registration_popped: bool,
    registration_relink: Option<u32>,
}

impl Delivery32 {
    fn return_is_live(&self, thread: u32, esp: u32) -> bool {
        self.thread == thread
            && self
                .active
                .is_none_or(|active| !active.registration_popped || esp <= active.return_esp)
    }
}
#[derive(Clone, Copy)]
pub(in crate::process) struct Unwind32 {
    target: u32,
    return_value: u32,
}

#[cfg(test)]
mod tests;
