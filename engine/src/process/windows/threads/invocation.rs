use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum ThreadInvocationCompletion32 {
    User32 { sender: u32, continuation: u64 },
    Ole32 { sender: u32, continuation: u64 },
    Apc,
    Initialize,
    Fls { operation: u64 },
    MultimediaTimer(crate::windows::winmm::timers::MultimediaTimerCallback32),
}
#[derive(Clone)]
pub(super) struct ThreadInvocation32 {
    pub id: u64,
    pub target: u32,
    pub procedure: GuestAddress,
    pub arguments: [u32; 16],
    pub count: usize,
    pub completion: ThreadInvocationCompletion32,
    pub timer: Option<crate::windows::synchronization::TimerApc32>,
    pub activation: Option<u32>,
    pub managed: Option<crate::windows::activation::CapturedActivation32>,
}
pub(in crate::process) struct PreparedThreadInvocation32 {
    invocation: ThreadInvocation32,
    signals: Option<crate::windows::synchronization::PreparedSignals32>,
    sent: Option<crate::process::windows::user32::PreparedUser32Sent32>,
    activation: Option<crate::process::windows::activation::PreparedActivationCapture32>,
}
impl ThreadRuntime32 {
    pub(in crate::process) fn managed_activation_capture_capacity(&self) -> usize {
        self.limits.apcs as usize + self.limits.threads as usize * 32
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_thread_invocation(
        &self,
        target: u32,
        procedure: GuestAddress,
        arguments: &[u32],
        completion: ThreadInvocationCompletion32,
    ) -> Result<PreparedThreadInvocation32, HostError> {
        self.prepare_thread_invocation_with_activation(
            target, procedure, arguments, completion, None, false,
        )
    }
    pub(in crate::process) fn prepare_thread_invocation_with_activation(
        &self,
        target: u32,
        procedure: GuestAddress,
        arguments: &[u32],
        completion: ThreadInvocationCompletion32,
        activation: Option<u32>,
        owned: bool,
    ) -> Result<PreparedThreadInvocation32, HostError> {
        self.memory()?;
        let thread = self.threads.info(target).map_err(HostError::Thread)?;
        if thread.state == ThreadState32::Terminated {
            return Err(HostError::Thread(ThreadError32::InvalidHandle));
        }
        if arguments.len() > 16 {
            return Err(HostError::Thread(ThreadError32::InvalidParameter));
        }
        if matches!(
            completion,
            ThreadInvocationCompletion32::User32 { .. }
                | ThreadInvocationCompletion32::Ole32 { .. }
                | ThreadInvocationCompletion32::MultimediaTimer(_)
        ) {
            self.memory()?
                .check_span(procedure, 1, Access::Execute)
                .map_err(HostError::Memory)?;
        }
        let sent = if matches!(completion, ThreadInvocationCompletion32::User32 { .. }) {
            Some(
                self.user32
                    .as_ref()
                    .ok_or(HostError::InvalidRequest)?
                    .prepare_sent_change(target, 1)
                    .map_err(HostError::User32)?,
            )
        } else {
            None
        };
        let signals = if completion == ThreadInvocationCompletion32::Apc {
            Some(
                self.synchronization()?
                    .prepare_apc_state(target, true)
                    .map_err(HostError::Synchronization)?,
            )
        } else if let Some(sent) = sent.as_ref() {
            Some(self.prepare_thread_sent_queue(sent)?)
        } else {
            None
        };
        if self.threads.invocations.len() >= self.threads.limits.apcs as usize {
            return Err(HostError::Thread(ThreadError32::Capacity));
        }
        let id = self
            .threads
            .next_invocation
            .checked_add(1)
            .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
        let mut args = [0; 16];
        args[..arguments.len()].copy_from_slice(arguments);
        let captured = activation;
        let activation = if owned {
            None
        } else {
            Some(if let Some(handle) = captured {
                self.prepare_activation_capture_handle(handle)?
            } else {
                self.prepare_activation_capture()?
            })
        };
        Ok(PreparedThreadInvocation32 {
            activation,
            signals,
            sent,
            invocation: ThreadInvocation32 {
                id,
                target,
                procedure,
                arguments: args,
                count: arguments.len(),
                completion,
                timer: None,
                activation: if owned { captured } else { None },
                managed: None,
            },
        })
    }
    pub(in crate::process) fn commit_thread_invocation(
        &mut self,
        prepared: PreparedThreadInvocation32,
    ) -> u64 {
        let mut invocation = prepared.invocation;
        assert_eq!(
            self.threads.next_invocation.checked_add(1),
            Some(invocation.id)
        );
        assert!(self.threads.invocations.len() < self.threads.invocations.capacity());
        assert_ne!(
            self.threads.info(invocation.target).unwrap().state,
            ThreadState32::Terminated
        );
        if let Some(signals) = prepared.signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        if let Some(sent) = prepared.sent {
            self.user32.as_mut().unwrap().commit_sent_change(sent);
        }
        if let Some(activation) = prepared.activation {
            invocation.activation = Some(self.commit_activation_capture(activation));
        }
        self.threads.next_invocation = invocation.id;
        let id = invocation.id;
        self.threads.invocations.push(invocation);
        id
    }
    fn prepare_thread_sent_queue(
        &self,
        sent: &crate::process::windows::user32::PreparedUser32Sent32,
    ) -> Result<crate::windows::synchronization::PreparedSignals32, HostError> {
        let (current, new, generation) = sent.queue_status();
        self.synchronization()?
            .prepare_messages_for_thread(sent.thread(), current, new, generation)
            .map_err(HostError::Synchronization)
    }
    pub(in crate::process) fn thread_invocation_activation_holds(
        &self,
        thread: u32,
    ) -> Result<Vec<u32>, HostError> {
        let record = self
            .threads
            .threads
            .iter()
            .find(|record| record.id == thread)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        let mut holds = Vec::new();
        holds
            .try_reserve_exact(self.threads.invocations.len() + record.active_invocations.len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        holds.extend(
            self.threads
                .invocations
                .iter()
                .filter(|invocation| invocation.target == thread
                    || matches!(invocation.completion, ThreadInvocationCompletion32::Ole32 { sender, .. } if sender == thread))
                .filter_map(|invocation| invocation.activation)
                .filter(|&handle| handle != 0),
        );
        holds.extend(
            record
                .active_invocations
                .iter()
                .filter_map(|invocation| invocation.activation)
                .map(crate::windows::activation::ActivationScope32::retained_handle)
                .filter(|&handle| handle != 0),
        );
        Ok(holds)
    }
    pub(in crate::process) fn has_pending_thread_invocation(&self, thread: u32) -> bool {
        self.threads.invocations.iter().any(|invocation| {
            invocation.target == thread
                && matches!(
                    invocation.completion,
                    ThreadInvocationCompletion32::User32 { .. }
                )
        })
    }
}

pub(super) struct ActiveThreadInvocation32 {
    pub id: u64,
    pub return_esp: u32,
    pub completion: ThreadInvocationCompletion32,
    pub activation: Option<crate::windows::activation::ActivationScope32>,
    pub _activation_capture: Option<crate::windows::activation::CapturedActivation32>,
    pub saved: super::saved::SavedThread32,
}
pub(in crate::process) struct PreparedThreadApc32 {
    entry: PreparedInvocationEntry32,
    queue_index: usize,
    signals: crate::windows::synchronization::PreparedSignals32,
}
struct PreparedInvocationEntry32 {
    invocation: ThreadInvocation32,
    thread_index: usize,
    frame: crate::windows::CallbackFrame32,
    state: [u8; crate::abi::x86::STATE_SIZE],
    exit: [u8; crate::abi::x86::EXIT_SIZE],
    activation: Option<crate::process::windows::activation::PreparedActivationEntry32>,
}

impl EngineInstance {
    pub(super) fn begin_pending_thread_invocation(&mut self) -> Result<bool, HostError> {
        let current = self.threads.current_id();
        let thread_index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == current)
            .unwrap();
        let messages = self.threads.threads[thread_index].message_dispatch
            && self.waiting_user32_message().is_some();
        let alertable = self
            .synchronization
            .as_ref()
            .is_some_and(|runtime| runtime.active_alertable())
            || self.windows_file_wait_alertable()
            || (!self.threads.threads[thread_index].entered
                && self.threads.threads[thread_index].started
                && self.threads.threads[thread_index].lifecycle.is_none());
        let Some(queue_index) = self.threads.invocations.iter().position(|invocation| {
            invocation.target == current
                && match invocation.completion {
                    ThreadInvocationCompletion32::Apc => alertable,
                    ThreadInvocationCompletion32::User32 { .. } => messages,
                    ThreadInvocationCompletion32::Ole32 { .. } => {
                        self.com_apartment_invocation_eligible(current)
                    }
                    ThreadInvocationCompletion32::MultimediaTimer(_) => {
                        self.threads.threads[thread_index].started
                            && self.threads.threads[thread_index]
                                .active_invocations
                                .is_empty()
                    }
                    _ => false,
                }
        }) else {
            return Ok(false);
        };
        let invocation = self.threads.invocations[queue_index].clone();
        let sent = if matches!(
            invocation.completion,
            ThreadInvocationCompletion32::User32 { .. }
        ) {
            Some(
                self.user32
                    .as_ref()
                    .ok_or(HostError::InvalidRequest)?
                    .prepare_sent_change(current, -1)
                    .map_err(HostError::User32)?,
            )
        } else {
            None
        };
        let signals = if invocation.completion == ThreadInvocationCompletion32::Apc {
            let remains = self
                .threads
                .invocations
                .iter()
                .enumerate()
                .any(|(index, item)| {
                    index != queue_index
                        && item.target == current
                        && item.completion == ThreadInvocationCompletion32::Apc
                });
            Some(self.prepare_invocation_apc_state(&invocation, remains)?)
        } else if let Some(sent) = sent.as_ref() {
            Some(self.prepare_thread_sent_queue(sent)?)
        } else {
            None
        };
        self.enter_thread_invocation(invocation)?;
        if let Some(signals) = signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        if let Some(sent) = sent {
            self.user32.as_mut().unwrap().commit_sent_change(sent);
        }
        self.threads.invocations.remove(queue_index);
        Ok(true)
    }
    pub(super) fn enter_thread_invocation(
        &mut self,
        invocation: ThreadInvocation32,
    ) -> Result<(), HostError> {
        let state = crate::abi::x86::decode_state(
            &self.arena()[crate::abi::arena::STATE_OFFSET
                ..crate::abi::arena::STATE_OFFSET + crate::abi::x86::STATE_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        let prepared = self.prepare_invocation_entry(invocation, state)?;
        self.commit_invocation_entry(prepared);
        Ok(())
    }
    fn prepare_invocation_entry(
        &mut self,
        invocation: ThreadInvocation32,
        state: crate::cpu::x86::State32,
    ) -> Result<PreparedInvocationEntry32, HostError> {
        let current = self.threads.current_id();
        let thread_index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == current)
            .unwrap();
        if self.threads.threads[thread_index].active_invocations.len() == 32 {
            return Err(HostError::Thread(ThreadError32::Capacity));
        }
        self.threads.threads[thread_index]
            .active_invocations
            .try_reserve(1)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        let return_pc = self
            .module_gates()?
            .iter()
            .find(|gate| gate.id == super::INVOCATION_RETURN_ID)
            .map(|gate| gate.entry.0)
            .ok_or(HostError::Thread(ThreadError32::Unsupported))?;
        let frame = crate::windows::CallbackFrame32::prepare(
            state,
            invocation.procedure.0,
            return_pc,
            &invocation.arguments[..invocation.count],
        )
        .map_err(|_| HostError::Thread(ThreadError32::InvalidParameter))?;
        let mut state_bytes = [0; crate::abi::x86::STATE_SIZE];
        crate::abi::x86::encode_state_current(frame.state(), &mut state_bytes)
            .map_err(|_| HostError::Infrastructure)?;
        let mut exit_bytes = [0; crate::abi::x86::EXIT_SIZE];
        crate::abi::x86::encode_exit_v3(
            &crate::cpu::ExecutionExit {
                retired: 0,
                reason: crate::cpu::ExitReason::NeedCode,
            },
            &mut exit_bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;
        crate::process::cpu::context::ExtendedCpuState::capture(self.arena())?;
        if let ThreadInvocationCompletion32::Ole32 {
            sender,
            continuation,
        } = invocation.completion
        {
            self.prepare_com_apartment_entry(sender, continuation)?;
        }
        self.memory()?
            .ensure_transaction_versions(1)
            .map_err(HostError::Memory)?;
        for word in frame.writes() {
            self.memory()?
                .check_span(word.address, 4, Access::Write)
                .map_err(HostError::Memory)?;
        }
        let activation = if let Some(capture) = &invocation.managed {
            self.prepare_activation_entry_borrowed(capture.handle())?
        } else {
            self.prepare_activation_entry(invocation.activation)?
        };
        Ok(PreparedInvocationEntry32 {
            activation,
            invocation,
            thread_index,
            frame,
            state: state_bytes,
            exit: exit_bytes,
        })
    }
    fn commit_invocation_entry(&mut self, prepared: PreparedInvocationEntry32) {
        let PreparedInvocationEntry32 {
            invocation,
            thread_index,
            frame,
            state: state_bytes,
            exit: exit_bytes,
            activation,
        } = prepared;
        assert_eq!(self.threads.current_id(), invocation.target);
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(frame.writes())
            .expect("invocation stack writes preflighted before CPU publication");
        if invocation.completion == ThreadInvocationCompletion32::Apc {
            self.commit_windows_file_wait_apc();
        }
        let activation = self.commit_activation_entry(activation);
        let saved = self
            .take_windows_thread_context()
            .expect("validated thread CPU state is unchanged by stack writes");
        self.threads.threads[thread_index]
            .active_invocations
            .push(ActiveThreadInvocation32 {
                id: invocation.id,
                return_esp: frame.return_esp(),
                completion: invocation.completion,
                activation,
                _activation_capture: invocation.managed,
                saved,
            });
        self.arena.as_mut().get_mut()[crate::abi::arena::STATE_OFFSET
            ..crate::abi::arena::STATE_OFFSET + crate::abi::x86::STATE_SIZE]
            .copy_from_slice(&state_bytes);
        self.arena.as_mut().get_mut()[crate::abi::arena::EXIT_OFFSET
            ..crate::abi::arena::EXIT_OFFSET + crate::abi::x86::EXIT_SIZE]
            .copy_from_slice(&exit_bytes);
        self.threads.threads[thread_index].state = ThreadState32::Running;
        if let ThreadInvocationCompletion32::Ole32 {
            sender,
            continuation,
        } = invocation.completion
        {
            self.enter_com_apartment_invocation(sender, continuation);
        }
    }
    pub(super) fn finish_thread_invocation(
        &mut self,
        frame: crate::windows::CallFrame32,
    ) -> Result<(), HostError> {
        let current = self.threads.current_id();
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == current)
            .unwrap();
        let active = self.threads.threads[index]
            .active_invocations
            .last()
            .ok_or(HostError::Thread(ThreadError32::Stale))?;
        if frame.state().registers[4] != active.return_esp {
            return Err(HostError::Thread(ThreadError32::InvalidParameter));
        }
        crate::process::cpu::context::ExtendedCpuState::capture(self.arena())?;
        let completion = active.completion;
        let activation = self.prepare_activation_scope_exit(active.activation)?;
        if completion == ThreadInvocationCompletion32::Initialize
            && self.threads.threads[index]
                .lifecycle
                .as_ref()
                .and_then(|phase| phase.active)
                != Some(active.id)
        {
            return Err(HostError::Thread(ThreadError32::Stale));
        }
        if let ThreadInvocationCompletion32::Fls { operation } = completion {
            self.threads
                .fls
                .context
                .check_callback(operation)
                .map_err(HostError::Thread)?;
        }
        if let ThreadInvocationCompletion32::Ole32 {
            sender,
            continuation,
        } = completion
        {
            self.check_com_apartment_invocation(sender, continuation)?;
        }
        let result = frame.state().registers[0];
        if let ThreadInvocationCompletion32::MultimediaTimer(identity) = completion {
            self.finish_multimedia_timer_callback(identity)?;
        }
        if let ThreadInvocationCompletion32::User32 {
            sender,
            continuation,
        } = completion
            && let Some(sender) = self
                .threads
                .threads
                .iter_mut()
                .find(|thread| thread.id == sender && thread.state != ThreadState32::Terminated)
        {
            sender
                .accept_remote_result(continuation, result)
                .map_err(HostError::User32)?;
            if sender.suspend_count == 0 {
                sender.state = ThreadState32::Runnable;
            }
        }
        self.commit_activation_scope_exit(activation);
        if let ThreadInvocationCompletion32::Ole32 {
            sender,
            continuation,
        } = completion
        {
            self.leave_com_apartment_invocation(sender, continuation);
            self.complete_com_apartment_invocation(sender, continuation, result)
                .expect("COM return identity was preflighted before publication");
        }
        let _ = self
            .take_windows_thread_context()
            .expect("invocation return context was preflighted");
        let active = self.threads.threads[index]
            .active_invocations
            .pop()
            .unwrap();
        self.restore_windows_thread_context(active.saved);
        if completion == ThreadInvocationCompletion32::Initialize {
            self.threads.threads[index]
                .lifecycle
                .as_mut()
                .expect("thread initialization owns this invocation")
                .calls
                .pop_front();
            self.threads.threads[index]
                .lifecycle
                .as_mut()
                .unwrap()
                .active = None;
        }
        if let ThreadInvocationCompletion32::Fls { operation } = completion {
            self.threads.fls.context.complete_callback(operation);
            self.advance_windows_fls_operation(operation)?;
            return Ok(());
        }
        self.threads.threads[index].state = if self.pending_call.is_some()
            || matches!(completion, ThreadInvocationCompletion32::MultimediaTimer(_))
            || (matches!(completion, ThreadInvocationCompletion32::Ole32 { .. })
                && self.com_apartment_host(current))
        {
            ThreadState32::Waiting
        } else {
            ThreadState32::Running
        };
        Ok(())
    }
}

impl EngineInstance {
    pub(in crate::process) fn prepare_windows_continue_apc(
        &mut self,
        candidate: &crate::windows::exceptions::context::ContextState32,
    ) -> Result<Option<PreparedThreadApc32>, HostError> {
        let current = self.threads.current_id();
        let Some(queue_index) = self.threads.invocations.iter().position(|invocation| {
            invocation.target == current
                && invocation.completion == ThreadInvocationCompletion32::Apc
        }) else {
            return Ok(None);
        };
        let remains = self
            .threads
            .invocations
            .iter()
            .enumerate()
            .any(|(index, invocation)| {
                index != queue_index
                    && invocation.target == current
                    && invocation.completion == ThreadInvocationCompletion32::Apc
            });
        let signals =
            self.prepare_invocation_apc_state(&self.threads.invocations[queue_index], remains)?;
        let entry = self.prepare_invocation_entry(
            self.threads.invocations[queue_index].clone(),
            candidate.state,
        )?;
        Ok(Some(PreparedThreadApc32 {
            entry,
            queue_index,
            signals,
        }))
    }
    pub(in crate::process) fn commit_windows_continue_apc(
        &mut self,
        prepared: Option<PreparedThreadApc32>,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        assert_eq!(
            self.threads.invocations[prepared.queue_index].id,
            prepared.entry.invocation.id
        );
        self.commit_invocation_entry(prepared.entry);
        self.threads.invocations.remove(prepared.queue_index);
        self.synchronization
            .as_mut()
            .unwrap()
            .context_mut()
            .commit_signals(prepared.signals);
    }
    pub(in crate::process) fn has_pending_thread_apc(&self, thread: u32) -> bool {
        self.threads.invocations.iter().any(|invocation| {
            invocation.target == thread
                && invocation.completion == ThreadInvocationCompletion32::Apc
        })
    }
    fn prepare_invocation_apc_state(
        &self,
        invocation: &ThreadInvocation32,
        remaining: bool,
    ) -> Result<crate::windows::synchronization::PreparedSignals32, HostError> {
        let context = self.synchronization()?;
        if let Some(timer) = invocation.timer {
            context.prepare_timer_apc_delivery(
                timer.object,
                timer.generation,
                invocation.target,
                remaining,
            )
        } else {
            context.prepare_apc_state(invocation.target, remaining)
        }
        .map_err(HostError::Synchronization)
    }
}
