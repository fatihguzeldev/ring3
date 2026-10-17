use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_current_segment_context},
    },
    cpu::x86::SegmentRegister,
    process::{CallError, EngineInstance, HostError},
    windows::thread::ThreadSchedule32,
};

pub(super) struct PreparedThreadSwitch32 {
    current_index: usize,
    target_index: usize,
    context: crate::cpu::x86::SegmentContext,
}

impl EngineInstance {
    pub fn windows_thread_info(&self, thread: u32) -> Result<ThreadInfo32, HostError> {
        self.memory()?;
        self.threads.info(thread).map_err(HostError::Thread)
    }
    pub fn pending_windows_thread_schedule(&self) -> Option<u32> {
        self.is_open()
            .then_some(self.threads.schedule_required)
            .flatten()
    }
    fn windows_thread_eligible(&self, index: usize) -> bool {
        let thread = &self.threads.threads[index];
        if thread.suspend_count != 0
            || self
                .process_exit_owner
                .is_some_and(|owner| owner != thread.id)
            || ((!thread.started || thread.lifecycle.is_some() || thread.exit_requested.is_some())
                && self
                    .loader_initialization_owner()
                    .is_some_and(|owner| owner != thread.id))
        {
            return false;
        }
        if matches!(
            thread.state,
            ThreadState32::Runnable | ThreadState32::Running
        ) {
            return true;
        }
        if thread.state != ThreadState32::Waiting {
            return false;
        }
        let initialization = thread.exit_requested.is_some()
            || thread
                .lifecycle
                .as_ref()
                .is_some_and(|phase| phase.active.is_none());
        let messages = thread.message_dispatch
            && thread.saved.as_ref().map_or_else(
                || self.waiting_user32_message().is_some(),
                |saved| saved.waiting_for_user32(),
            )
            && self.has_pending_thread_invocation(thread.id);
        let com = self.has_com_apartment_invocation(thread.id)
            && (self.com_apartment_invocation_eligible(thread.id)
                || thread
                    .saved
                    .as_ref()
                    .is_some_and(|saved| saved.waiting_for_user32()));
        let current_apc = thread.id == self.threads.current_id()
            && (self
                .synchronization
                .as_ref()
                .is_some_and(|runtime| runtime.active_alertable())
                || self.windows_file_wait_alertable())
            && self.has_pending_thread_apc(thread.id);
        let ready = thread.saved.as_ref().is_some_and(|saved| {
            saved.ready(
                self,
                self.synchronization
                    .as_ref()
                    .map_or(0, |runtime| runtime.context().now_ns()),
                thread.id,
            )
        });
        initialization
            || messages
            || com
            || self.com_abandoned_work_ready(thread.id)
            || current_apc
            || ready
            || self.has_pending_multimedia_callback(thread.id)
    }
    fn next_windows_thread(&self, current: usize, exclude_current: bool) -> Option<usize> {
        let count = self.threads.threads.len();
        let mut selected = None;
        for offset in 1..=count {
            let index = (current + offset) % count;
            if (exclude_current && index == current) || !self.windows_thread_eligible(index) {
                continue;
            }
            if selected.is_none_or(|prior: usize| {
                self.threads.threads[index].priority > self.threads.threads[prior].priority
            }) {
                selected = Some(index);
            }
        }
        selected
    }
    pub(super) fn prepare_windows_thread_yield(
        &self,
    ) -> Result<Option<PreparedThreadSwitch32>, HostError> {
        let current = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == self.threads.current_id())
            .unwrap();
        self.next_windows_thread(current, true)
            .map(|index| self.prepare_windows_thread_switch(index))
            .transpose()
    }
    pub(super) fn commit_windows_thread_yield(&mut self, prepared: PreparedThreadSwitch32) {
        let target = self.threads.threads[prepared.target_index].id;
        self.commit_windows_thread_switch(prepared);
        self.threads.schedule_required = Some(target);
    }
    pub fn schedule_windows_thread(&mut self) -> Result<ThreadSchedule32, HostError> {
        self.memory()?;
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let now = self.cpu_clock.monotonic_bounds()?.1;
        self.service_multimedia_timers(now)?;
        if self.synchronization.as_ref().is_some_and(|runtime| {
            runtime
                .context()
                .next_timer_deadline()
                .is_some_and(|due| due <= now)
        }) {
            let prepared = self.prepare_synchronization_clock(now)?;
            self.commit_synchronization_clock(prepared);
        }
        for _ in 0..=self.threads.threads.len() {
            self.resume_ready_thread_provider()?;
            self.resume_ready_thread_wait()?;
            self.resume_ready_user32_thread_wait()?;
            self.apply_deferred_thread_context()?;
            if self.resume_module_loader_wait()? {
                let current = self.threads.current_id();
                if let Some(thread) = self
                    .threads
                    .threads
                    .iter_mut()
                    .find(|thread| thread.id == current)
                {
                    thread.state = ThreadState32::Running;
                }
            }
            let current = self.threads.current_id();
            let current_index = self
                .threads
                .threads
                .iter()
                .position(|thread| thread.id == current)
                .unwrap();
            if self.threads.threads[current_index].state == ThreadState32::Running
                && self.pending_call.is_some()
                && self.pending_windows_callback().is_none()
            {
                self.threads.threads[current_index].state = ThreadState32::Waiting;
            }
            let now = self.cpu_clock.monotonic_bounds()?.1;
            for index in 0..self.threads.threads.len() {
                let thread = &self.threads.threads[index];
                if thread.state == ThreadState32::Waiting
                    && thread.suspend_count == 0
                    && thread
                        .saved
                        .as_ref()
                        .is_some_and(|saved| saved.ready(self, now, thread.id))
                {
                    self.threads.threads[index].state = ThreadState32::Runnable;
                }
            }
            let selected = if self.threads.schedule_required == Some(current)
                && self.windows_thread_eligible(current_index)
            {
                Some(current_index)
            } else {
                self.next_windows_thread(current_index, false)
            };
            if let Some(index) = selected {
                if index != current_index {
                    self.switch_windows_thread(index)?;
                }
                self.resume_ready_thread_provider()?;
                self.resume_ready_thread_wait()?;
                self.resume_ready_user32_thread_wait()?;
                self.resume_module_loader_wait()?;
                self.apply_deferred_thread_context()?;
                if self.cpu_event.is_none()
                    && !self.exceptions.thread_unhandled(self.threads.current_id())
                {
                    self.advance_windows_thread_initialization()?;
                }
                if self.threads.threads[index].state != ThreadState32::Terminated
                    && self.cpu_event.is_none()
                    && !self.exceptions.thread_unhandled(self.threads.current_id())
                {
                    self.begin_pending_thread_invocation()?;
                }
                let current = self.threads.current_id();
                if self.cpu_event.is_some() || self.exceptions.thread_unhandled(current) {
                    self.threads.schedule_required = None;
                    return Ok(ThreadSchedule32::CpuEvent { thread: current });
                }
                if self.pending_windows_callback().is_some() {
                    self.threads.schedule_required = None;
                    return Ok(ThreadSchedule32::WindowsCallback { thread: current });
                }
                if self.pending_call.is_none()
                    && self.threads.threads[index].state != ThreadState32::Terminated
                {
                    self.threads.threads[index].state = ThreadState32::Running;
                    if self.threads.threads[index].active_invocations.is_empty()
                        && self.threads.threads[index].started
                    {
                        self.threads.threads[index].entered = true;
                    }
                    self.threads.schedule_required = None;
                    return Ok(ThreadSchedule32::Running { thread: current });
                }
                if let Some(code) = self.exit_code {
                    self.threads.schedule_required = None;
                    return Ok(ThreadSchedule32::Exited { code });
                }
                if self.threads.threads[index].state != ThreadState32::Terminated {
                    self.threads.threads[index].state = ThreadState32::Waiting;
                }
                self.threads.schedule_required = None;
                continue;
            }
            break;
        }
        let live = self
            .threads
            .threads
            .iter()
            .any(|thread| thread.state != ThreadState32::Terminated);
        if !live {
            return Ok(ThreadSchedule32::Exited {
                code: self
                    .threads
                    .threads
                    .iter()
                    .find(|thread| thread.id == self.threads.current_id())
                    .unwrap()
                    .exit_code,
            });
        }
        let deadline = self
            .threads
            .threads
            .iter()
            .filter_map(|thread| thread.saved.as_ref().and_then(|saved| saved.deadline()))
            .chain(
                self.pending_windows_wait()
                    .and_then(|wait| wait.deadline_ns),
            )
            .chain(
                self.synchronization
                    .as_ref()
                    .and_then(|runtime| runtime.context().next_timer_deadline()),
            )
            .chain(self.active_windows_file_deadline())
            .chain(self.user32_timer_deadline())
            .chain(self.multimedia_timer_deadline())
            .min();
        self.threads.schedule_required = None;
        Ok(ThreadSchedule32::Waiting {
            deadline_ns: deadline,
        })
    }
    fn resume_ready_thread_provider(&mut self) -> Result<(), HostError> {
        if let Some(token) = self.ready_user32_image_reply_token() {
            self.resume_user32_image_call(self.key(), token)?;
        }
        if self.active_version_reply_ready() {
            let wait = self.pending_version_call()?.ok_or(HostError::Infrastructure)?;
            self.resume_version_call(wait.key, wait.token)?;
        }

        if self.active_activation_reply_ready() {
            let wait = self
                .pending_activation_call()
                .ok_or(HostError::Infrastructure)?;
            self.resume_activation_call(wait.epoch, wait.token)?;
        }
        self.resume_registered_com_loader_wait()?;
        self.resume_com_proxy_wait()?;
        self.service_abandoned_com_work()?;
        if self.winmm.active_thread_reply_ready() {
            self.resume_winmm_timer_call()?;
        }
        if self
            .dinput
            .as_ref()
            .and_then(|runtime| runtime.active_file_request())
            .is_some_and(|request| self.provider_file_request_ready(request))
        {
            let token = self
                .pending_dinput_file_token()
                .ok_or(HostError::Infrastructure)?;
            self.resume_dinput_file_call(self.key(), token)?;
        }
        if let Some(pending) = self.pending_call.as_ref() {
            let (owner, token) = (pending.owner, pending.token);
            self.retry_windows_callback_reply(owner, token)?;
        }
        let now = self.cpu_clock.monotonic_bounds()?.1;
        if self.files.as_ref().is_some_and(|runtime| {
            runtime.active_thread_reply_ready(now, |id| self.threads.is_runnable(id))
        }) {
            let wait = self.pending_file_call().ok_or(HostError::Infrastructure)?;
            self.resume_file_call(self.key(), wait.token)?;
        }
        if self
            .registry
            .as_ref()
            .is_some_and(|runtime| runtime.active_thread_reply_ready())
        {
            let wait = self
                .pending_registry_call()
                .ok_or(HostError::Infrastructure)?;
            self.resume_registry_call(self.key(), wait.token)?;
        }
        if self
            .d3d8_reply
            .as_ref()
            .zip(self.d3d8.as_ref())
            .is_some_and(|(reply, runtime)| reply.ready(runtime))
        {
            let pending = self
                .pending_call
                .as_ref()
                .ok_or(HostError::Infrastructure)?;
            let crate::cpu::ExitReason::Gate { id } = crate::abi::x86::decode_exit(&pending.exit)
                .map_err(|_| HostError::Infrastructure)?
                .reason
            else {
                return Err(HostError::Infrastructure);
            };
            let (owner, token, frame) = (pending.owner, pending.token, pending.frame);
            match self.try_complete_d3d8_call(owner, token, id, frame) {
                Ok(_) | Err(HostError::Call(CallError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        if self.pending_call.is_none() || self.pending_windows_callback().is_some() {
            let current = self.threads.current_id();
            let worker_idle = (self
                .threads
                .multimedia_worker
                .is_some_and(|(id, _)| id == current)
                && !self.has_pending_multimedia_callback(current)
                || self.com_apartment_host(current) && !self.has_com_apartment_invocation(current))
                && self
                    .threads
                    .threads
                    .iter()
                    .find(|thread| thread.id == current)
                    .is_some_and(|thread| thread.active_invocations.is_empty());
            if let Some(thread) = self.threads.threads.iter_mut().find(|thread| {
                thread.id == current
                    && thread.state != ThreadState32::Terminated
                    && thread.suspend_count == 0
                    && !worker_idle
            }) {
                thread.state = ThreadState32::Running;
            }
        }
        Ok(())
    }
    pub(super) fn switch_windows_thread(&mut self, index: usize) -> Result<(), HostError> {
        let prepared = self.prepare_windows_thread_switch(index)?;
        self.commit_windows_thread_switch(prepared);
        Ok(())
    }
    fn prepare_windows_thread_switch(
        &self,
        index: usize,
    ) -> Result<PreparedThreadSwitch32, HostError> {
        crate::process::cpu::context::ExtendedCpuState::capture(self.arena())?;
        let current_index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == self.threads.current_id())
            .unwrap();
        let saved = self.threads.threads[index]
            .saved
            .as_ref()
            .ok_or(HostError::Thread(ThreadError32::Stale))?;
        let context = decode_current_segment_context(
            &saved.arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            Some(
                &saved.arena[SEGMENT_BASES_RECORD_OFFSET
                    ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE],
            ),
        )
        .map_err(|_| HostError::Infrastructure)?;
        let fs = context.state.selectors[SegmentRegister::Fs.index()];
        if fs & !3 != 0 && self.segment_descriptors.raw_descriptor(fs).is_none() {
            return Err(HostError::Thread(ThreadError32::Stale));
        }
        Ok(PreparedThreadSwitch32 {
            current_index,
            target_index: index,
            context,
        })
    }
    fn commit_windows_thread_switch(&mut self, prepared: PreparedThreadSwitch32) {
        let PreparedThreadSwitch32 {
            current_index,
            target_index: index,
            context,
        } = prepared;
        assert_eq!(
            self.threads.threads[current_index].id,
            self.threads.current_id()
        );
        let fs = context.state.selectors[SegmentRegister::Fs.index()];
        let epoch = self.cpu_event.as_ref().map_or(0, |event| event.epoch);
        #[cfg(any(target_arch = "wasm32", test))]
        let epoch = epoch.max(self.detection_epoch);
        let current = self
            .take_windows_thread_context()
            .expect("thread CPU state was validated before context exchange");
        self.threads.threads[current_index].duration_epoch = self.threads.threads[current_index]
            .duration_epoch
            .max(epoch);
        let saved = self.threads.threads[index].saved.take().unwrap();
        if self.threads.threads[current_index].state != ThreadState32::Terminated {
            self.threads.threads[current_index].saved = Some(current);
        }
        if self.threads.threads[current_index].state == ThreadState32::Running {
            self.threads.threads[current_index].state = ThreadState32::Runnable;
        }
        self.threads.current = self.threads.threads[index].id;
        if fs & !3 != 0 {
            self.segment_descriptors
                .update_base(fs, context.state.bases[SegmentRegister::Fs.index()])
                .expect("saved FS descriptor was preflighted");
        }
        self.restore_windows_thread_context(saved);
        if let Some(sync) = self.synchronization.as_mut() {
            sync.context_mut()
                .set_current_thread(self.threads.current_id())
                .expect("thread IDs are nonzero");
        }
    }
    fn resume_ready_user32_thread_wait(&mut self) -> Result<(), HostError> {
        if self
            .user32
            .as_ref()
            .is_some_and(|context| context.active_thread_wait_ready())
            && !self.has_pending_thread_invocation(self.threads.current_id())
            && let Some(continuation) = self.waiting_user32_message()
        {
            self.resume_user32_wait(self.key(), continuation)?;
            let current = self.threads.current_id();
            if self.pending_call.is_none() {
                self.threads
                    .threads
                    .iter_mut()
                    .find(|thread| thread.id == current)
                    .unwrap()
                    .state = ThreadState32::Running;
            }
        }
        Ok(())
    }
    fn resume_ready_thread_wait(&mut self) -> Result<(), HostError> {
        let ready = self.synchronization.as_ref().is_some_and(|runtime| {
            runtime.active_ready()
                && !(runtime.active_alertable()
                    && self.has_pending_thread_apc(self.threads.current_id()))
        });
        if ready {
            self.finish_synchronization_reply()?;
            let current = self.threads.current_id();
            if let Some(thread) = self
                .threads
                .threads
                .iter_mut()
                .find(|thread| thread.id == current)
                && thread.state == ThreadState32::Waiting
            {
                thread.state = ThreadState32::Running;
            }
        }
        Ok(())
    }
}
