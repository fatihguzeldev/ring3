mod calls;
mod com;
mod context;
mod cpu_context;
mod creation;
mod fls;
mod initialization;
mod invocation;
mod lifetime;
mod multimedia;
mod saved;
mod scheduling;
mod stack;
mod static_tls;
mod timers;
mod tls;
mod utilities;
pub(in crate::process) use timers::PreparedThreadTimers32;

use crate::{
    memory::PageRange,
    windows::thread::{ThreadError32, ThreadInfo32, ThreadLimits32, ThreadState32},
};

struct ThreadRecord32 {
    id: u32,
    teb: Option<u32>,
    stack: Option<PageRange>,
    stack_reserved: Option<PageRange>,
    state: ThreadState32,
    suspend_count: u32,
    exit_code: u32,
    reserved: Vec<PageRange>,
    message_dispatch: bool,
    tls_expansion: Option<PageRange>,
    tls_values: Vec<u32>,
    tls_array: Option<PageRange>,
    static_tls: Vec<static_tls::ThreadStaticTls32>,
    saved: Option<saved::SavedThread32>,
    active_invocations: Vec<invocation::ActiveThreadInvocation32>,
    started: bool,
    entered: bool,
    lifecycle: Option<initialization::ThreadInitialization32>,
    exit_requested: Option<u32>,
    priority: i32,
    created_filetime: u64,
    exited_filetime: u64,
    user_ns: u64,
    duration_epoch: u64,
    context_override: Option<cpu_context::ThreadContextOverride32>,
}
pub(in crate::process) struct ThreadRuntime32 {
    limits: ThreadLimits32,
    threads: Vec<ThreadRecord32>,
    current: u32,
    next_id: u32,
    tls_allocated: [u64; 17],
    invocations: Vec<invocation::ThreadInvocation32>,
    next_invocation: u64,
    schedule_required: Option<u32>,
    multimedia_worker: Option<(u32, u32)>,
    com_hosts: Vec<com::ComHost32>,
    fls: fls::FlsRuntime32,
}

#[cfg(test)]
mod tests;

pub(in crate::process) use invocation::{PreparedThreadInvocation32, ThreadInvocationCompletion32};

pub(in crate::process) use creation::{INVOCATION_RETURN_ID, THREAD_RETURN_ID};
pub(in crate::process) use stack::{PreparedStackOverflow32, StackGuardOutcome32};
