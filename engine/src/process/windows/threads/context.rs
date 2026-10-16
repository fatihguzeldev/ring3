use super::*;

pub(in crate::process) struct PreparedPrimaryThread32 {
    teb: u32,
    stack: PageRange,
}

impl ThreadRuntime32 {
    pub(in crate::process) fn record_primary_creation(&mut self, filetime: u64) {
        self.threads[0].created_filetime = filetime;
    }
    pub(in crate::process) fn new(limits: ThreadLimits32) -> Result<Self, ThreadError32> {
        if limits.threads == 0
            || limits.threads > 4096
            || limits.handles == 0
            || limits.handles > 65536
            || limits.apcs == 0
            || limits.apcs > 65536
            || limits.stack_pages == 0
            || limits.stack_pages > 4096
            || limits.stack_reserve_pages < limits.stack_pages
            || limits.stack_reserve_pages > 65536
        {
            return Err(ThreadError32::InvalidLimits);
        }
        let mut threads = Vec::new();
        threads
            .try_reserve_exact(limits.threads as usize)
            .map_err(|_| ThreadError32::Allocation)?;
        threads.push(ThreadRecord32 {
            id: 1,
            teb: None,
            stack: None,
            stack_reserved: None,
            state: ThreadState32::Running,
            suspend_count: 0,
            exit_code: 259,
            reserved: Vec::new(),
            message_dispatch: false,
            tls_expansion: None,
            tls_values: Vec::new(),
            tls_array: None,
            static_tls: Vec::new(),
            saved: None,
            active_invocations: Vec::new(),
            started: true,
            entered: true,
            lifecycle: None,
            exit_requested: None,
            priority: 0,
            created_filetime: 0,
            exited_filetime: 0,
            user_ns: 0,
            duration_epoch: 0,
            context_override: None,
        });
        let mut invocations = Vec::new();
        invocations
            .try_reserve_exact(limits.apcs as usize)
            .map_err(|_| ThreadError32::Allocation)?;
        Ok(Self {
            limits,
            threads,
            current: 1,
            next_id: 2,
            tls_allocated: [0; 17],
            invocations,
            next_invocation: 0,
            schedule_required: None,
            multimedia_worker: None,
            fls: fls::FlsRuntime32::new(128, limits.threads)?,
        })
    }
    pub(in crate::process) fn retire_process_threads(&mut self, code: u32) {
        for thread in &mut self.threads {
            self.fls.retire_thread(thread.id);
            if thread.state != ThreadState32::Terminated {
                thread.exit_code = code;
            }
            thread.state = ThreadState32::Terminated;
            thread.saved = None;
            thread.active_invocations.clear();
            thread.lifecycle = None;
            thread.exit_requested = None;
        }
        self.invocations.clear();
        self.schedule_required = None;
        self.multimedia_worker = None;
    }
    pub(in crate::process) fn is_last_live(&self) -> bool {
        self.threads
            .iter()
            .filter(|thread| {
                thread.state != ThreadState32::Terminated
                    && !self
                        .multimedia_worker
                        .is_some_and(|(id, _)| id == thread.id)
            })
            .count()
            == 1
    }
    pub(in crate::process) fn current_id(&self) -> u32 {
        self.current
    }
    pub(in crate::process) fn is_runnable(&self, id: u32) -> bool {
        self.threads.iter().any(|thread| {
            thread.id == id
                && thread.suspend_count == 0
                && matches!(
                    thread.state,
                    ThreadState32::Running | ThreadState32::Runnable
                )
        })
    }
    pub(in crate::process) fn handle_limit(&self) -> usize {
        self.limits.handles as usize
    }
    pub(in crate::process) fn current_teb(&self) -> Option<u32> {
        self.threads
            .iter()
            .find(|thread| thread.id == self.current)
            .and_then(|thread| thread.teb)
    }
    pub(in crate::process) fn reservations(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.threads
            .iter()
            .flat_map(|thread| thread.reserved.iter().copied())
    }
    pub(in crate::process) fn prepare_primary_thread(
        &self,
        teb: u32,
        stack: PageRange,
    ) -> Result<PreparedPrimaryThread32, ThreadError32> {
        let primary = self.threads.first().ok_or(ThreadError32::Stale)?;
        if self.current != 1
            || primary.id != 1
            || primary.teb.is_some()
            || teb == 0
            || !teb.is_multiple_of(crate::memory::PAGE_SIZE)
        {
            return Err(ThreadError32::Busy);
        }
        Ok(PreparedPrimaryThread32 { teb, stack })
    }
    pub(in crate::process) fn commit_primary_thread(&mut self, prepared: PreparedPrimaryThread32) {
        let primary = self
            .threads
            .first_mut()
            .expect("primary thread exists before startup publication");
        assert_eq!(self.current, 1);
        assert!(primary.teb.is_none());
        primary.teb = Some(prepared.teb);
        primary.stack = Some(prepared.stack);
    }
    pub(in crate::process) fn reset_primary_thread(&mut self) {
        for thread in &self.threads {
            self.fls.retire_thread(thread.id);
        }
        self.threads.truncate(1);
        if let Some(primary) = self.threads.first_mut() {
            primary.teb = None;
            primary.stack = None;
            primary.stack_reserved = None;
            primary.state = ThreadState32::Running;
            primary.suspend_count = 0;
            primary.exit_code = 259;
            primary.message_dispatch = false;
            primary.tls_expansion = None;
            primary.tls_values.clear();
            primary.tls_array = None;
            primary.static_tls.clear();
            primary.saved = None;
            primary.active_invocations.clear();
            primary.started = true;
            primary.entered = true;
            primary.lifecycle = None;
            primary.exit_requested = None;
            primary.priority = 0;
            primary.created_filetime = 0;
            primary.exited_filetime = 0;
            primary.user_ns = 0;
            primary.duration_epoch = 0;
            primary.context_override = None;
        }
        self.current = 1;
        self.tls_allocated.fill(0);
        self.invocations.clear();
        self.schedule_required = None;
    }
    pub(in crate::process) fn info(&self, id: u32) -> Result<ThreadInfo32, ThreadError32> {
        let thread = self
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .ok_or(ThreadError32::InvalidHandle)?;
        Ok(ThreadInfo32 {
            id: thread.id,
            state: thread.state,
            teb: thread.teb.unwrap_or(0),
            stack_base: thread
                .stack
                .map_or(0, |stack| stack.first as u32 * crate::memory::PAGE_SIZE),
            stack_pages: thread.stack.map_or(0, |stack| stack.count as u32),
            suspend_count: thread.suspend_count,
            exit_code: thread.exit_code,
        })
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn check_thread_message_dispatch(
        &self,
        thread: u32,
    ) -> Result<(), crate::process::HostError> {
        self.memory()?;
        let thread = self
            .threads
            .threads
            .iter()
            .find(|record| record.id == thread)
            .ok_or(crate::process::HostError::Thread(
                ThreadError32::InvalidHandle,
            ))?;
        if thread.state == ThreadState32::Terminated {
            return Err(crate::process::HostError::Thread(
                ThreadError32::InvalidHandle,
            ));
        }
        Ok(())
    }
    pub(in crate::process) fn commit_thread_message_dispatch(
        &mut self,
        thread: u32,
        allowed: bool,
    ) {
        let thread = self
            .threads
            .threads
            .iter_mut()
            .find(|record| record.id == thread)
            .expect("message dispatch thread was preflighted");
        assert_ne!(thread.state, ThreadState32::Terminated);
        thread.message_dispatch = allowed;
    }
    pub(in crate::process) fn set_thread_message_dispatch(
        &mut self,
        thread: u32,
        allowed: bool,
    ) -> Result<(), crate::process::HostError> {
        self.check_thread_message_dispatch(thread)?;
        self.commit_thread_message_dispatch(thread, allowed);
        Ok(())
    }
}
