use super::*;
use crate::process::{EngineInstance, HostError, windows::ole32::PreparedComThreadExit32};

impl EngineInstance {
    pub(super) fn terminate_windows_thread(&mut self, id: u32, code: u32) -> Result<(), HostError> {
        self.memory()?;
        let exited_filetime = self.windows_filetime()?;
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == id)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        if self.threads.threads[index].state == ThreadState32::Terminated {
            return Ok(());
        }
        if id == self.threads.current_id() {
            crate::process::cpu::context::ExtendedCpuState::capture(self.arena())?;
        }
        self.check_automation_thread_scratch(id)?;
        let record = &self.threads.threads[index];
        if id == self.threads.current_id() {
            if let Some(user32) = self.user32.as_ref() {
                user32
                    .check_owned_thread_scratch(self.memory()?)
                    .map_err(HostError::User32)?;
            }
            if let Some(dinput) = self.dinput.as_ref() {
                dinput.check_active_thread_scratch(self.memory()?)?;
            }
        } else if let Some(saved) = record.saved.as_ref() {
            saved.check_owned_scratch(self.memory()?)?;
        }
        for invocation in &record.active_invocations {
            invocation.saved.check_owned_scratch(self.memory()?)?;
        }

        let scratch = if id == self.threads.current_id() {
            self.user32
                .as_ref()
                .map_or(0, |context| context.thread_scratch_ranges().count())
                + self
                    .dinput
                    .as_ref()
                    .map_or(0, |context| context.active_thread_scratch_ranges().count())
        } else {
            record
                .saved
                .as_ref()
                .map_or(0, |saved| saved.scratch_ranges().count())
        };
        let extra = record
            .active_invocations
            .iter()
            .map(|invocation| invocation.saved.scratch_ranges().count())
            .sum::<usize>();
        let resource_ranges = self
            .resources
            .as_ref()
            .map(|runtime| runtime.thread_exit_ranges(self.memory().unwrap(), id))
            .transpose()
            .map_err(HostError::Resource)?
            .unwrap_or_default();
        let nls_ranges = self
            .nls
            .thread_exit_ranges(self.memory()?, id)
            .map_err(HostError::Nls)?;
        let gdi_ranges = self
            .gdi
            .as_ref()
            .map(|gdi| gdi.thread_scratch_ranges(self.memory().unwrap(), id))
            .transpose()
            .map_err(HostError::Gdi)?
            .unwrap_or_default();
        let activation = self.prepare_activation_thread_exit(id)?;
        let com: PreparedComThreadExit32 = self.prepare_com_thread_exit(id)?;
        let automation_arrays: crate::process::windows::automation::PreparedAutomationArrays32 =
            self.prepare_automation_array_retirement(Some(id))?;
        let automation_allocations = self.automation_thread_temporary_allocations(id)?;
        let automation_retirement =
            self.prepare_windows_runtime_retirement(&automation_allocations)?;
        let capacity = self
            .windows_memory
            .committed_ranges()
            .count()
            .checked_add(scratch)
            .and_then(|count| count.checked_add(extra))
            .and_then(|count| count.checked_add(resource_ranges.len()))
            .and_then(|count| count.checked_add(nls_ranges.len()))
            .and_then(|count| count.checked_add(gdi_ranges.len()))
            .and_then(|count| count.checked_add(self.automation_thread_scratch_ranges(id).count()))
            .and_then(|count| {
                count.checked_add(
                    activation
                        .as_ref()
                        .map_or(0, |receipt| receipt.ranges().len()),
                )
            })
            .and_then(|count| count.checked_add(com.ranges().len()))
            .and_then(|count| {
                count.checked_add(
                    record
                        .static_tls
                        .iter()
                        .filter(|entry| entry.external)
                        .count(),
                )
            })
            .ok_or(HostError::Thread(ThreadError32::Capacity))?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(capacity)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        for range in self.windows_memory.committed_ranges() {
            if record.reserved.iter().any(|reserved| {
                reserved.first <= range.first
                    && range.first + range.count <= reserved.first + reserved.count
            }) {
                ranges.push(range);
            }
        }
        for &entry in record.static_tls.iter().filter(|entry| entry.external) {
            self.check_thread_tls_mapping(entry)?;
            ranges.push(entry.range);
        }
        if id == self.threads.current_id() {
            if let Some(user32) = self.user32.as_ref() {
                ranges.extend(user32.thread_scratch_ranges());
            }
            if let Some(dinput) = self.dinput.as_ref() {
                ranges.extend(dinput.active_thread_scratch_ranges());
            }
        } else if let Some(saved) = record.saved.as_ref() {
            ranges.extend(saved.scratch_ranges());
        }
        for invocation in &record.active_invocations {
            ranges.extend(invocation.saved.scratch_ranges());
        }
        ranges.extend(resource_ranges);
        ranges.extend(nls_ranges);
        ranges.extend(gdi_ranges);
        ranges.extend(self.automation_thread_scratch_ranges(id));
        if let Some(receipt) = &activation {
            ranges.extend_from_slice(receipt.ranges());
        }
        ranges.extend_from_slice(com.ranges());
        ranges.sort_unstable_by_key(|range| range.first);
        for pair in ranges.windows(2) {
            if pair[0].first + pair[0].count > pair[1].first {
                return Err(HostError::Thread(ThreadError32::Stale));
            }
        }
        let mut remote = Vec::new();
        remote
            .try_reserve_exact(record.active_invocations.len() + self.threads.invocations.len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        for completion in record
            .active_invocations
            .iter()
            .map(|invocation| invocation.completion)
            .chain(
                self.threads
                    .invocations
                    .iter()
                    .filter(|invocation| invocation.target == id)
                    .map(|invocation| invocation.completion),
            )
        {
            if let ThreadInvocationCompletion32::User32 {
                sender,
                continuation,
            } = completion
            {
                if sender == id {
                    continue;
                }
                if let Some(thread) =
                    self.threads.threads.iter().find(|thread| {
                        thread.id == sender && thread.state != ThreadState32::Terminated
                    })
                {
                    thread
                        .check_remote_result(continuation)
                        .map_err(HostError::User32)?;
                    remote.push((sender, continuation));
                }
            }
        }
        let old_windows = self.user32_window_identities();
        let flags = self.user32.as_ref().map(|context| {
            let kept = |window: Option<crate::windows::window::WindowHandle32>| {
                window.is_some_and(|window| {
                    context
                        .windows()
                        .get(window)
                        .is_ok_and(|record| record.description.thread_id != id)
                })
            };
            (
                kept(context.windows().focus()),
                kept(context.windows().capture()),
            )
        });
        let input = flags
            .map(|(focus, capture)| self.prepare_windows_input_flags(focus, capture))
            .transpose()?
            .flatten();
        let window_event = if flags.is_some_and(|(focus, capture)| {
            focus != (old_windows.0 != 0) || capture != (old_windows.1 != 0)
        }) {
            self.prepare_user32_window_event()?
        } else {
            None
        };
        let user32 = self
            .user32
            .as_ref()
            .map(|context| context.prepare_thread_termination(id))
            .transpose()
            .map_err(HostError::User32)?;
        let mut saved_files = Vec::new();
        saved_files
            .try_reserve_exact(record.active_invocations.len() + 1)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        saved_files.extend(
            record
                .saved
                .iter()
                .chain(
                    record
                        .active_invocations
                        .iter()
                        .map(|invocation| &invocation.saved),
                )
                .filter_map(|saved| saved.files_reply()),
        );
        let files = self
            .files
            .as_ref()
            .map(|runtime| {
                runtime.prepare_thread_exit(
                    self.memory().unwrap(),
                    id,
                    &ranges,
                    &saved_files,
                    id == self.threads.current_id(),
                )
            })
            .transpose()
            .map_err(HostError::File)?;
        let mut bindings = self
            .registry
            .as_ref()
            .map(|runtime| runtime.thread_exit_bindings(id))
            .transpose()
            .map_err(HostError::Registry)?
            .unwrap_or_default();
        if let Some(files) = files.as_ref() {
            bindings
                .try_reserve_exact(files.bindings().len())
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            bindings.extend_from_slice(files.bindings());
        }
        let file_words = files.as_ref().map_or(&[][..], |files| files.words());
        let mut merged_words = Vec::new();
        merged_words
            .try_reserve_exact(file_words.len() + automation_arrays.words().len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        merged_words.extend_from_slice(file_words);
        merged_words.extend_from_slice(automation_arrays.words());
        merged_words.sort_unstable_by_key(|word| word.address.0);
        if merged_words
            .windows(2)
            .any(|pair| u64::from(pair[0].address.0) + 4 > u64::from(pair[1].address.0))
        {
            return Err(HostError::Thread(ThreadError32::Stale));
        }
        let words = merged_words.as_slice();
        let mut saved_dinput = Vec::new();
        saved_dinput
            .try_reserve_exact(record.active_invocations.len() + 1)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        saved_dinput.extend(
            record
                .saved
                .iter()
                .chain(
                    record
                        .active_invocations
                        .iter()
                        .map(|invocation| &invocation.saved),
                )
                .filter_map(|saved| saved.dinput_reply()),
        );
        let dinput = self
            .dinput
            .as_ref()
            .map(|runtime| {
                runtime.prepare_thread_exit(
                    self.memory().unwrap(),
                    &saved_dinput,
                    id == self.threads.current_id(),
                    &ranges,
                    words,
                )
            })
            .transpose()
            .map_err(|error| {
                HostError::DirectInput(
                    if error == crate::input::dinput::DirectInputResult32::OUT_OF_MEMORY {
                        crate::process::windows::dinput::DirectInputError32::Allocation
                    } else {
                        crate::process::windows::dinput::DirectInputError32::InvalidRequest
                    },
                )
            })?;
        if let Some(prepared) = &dinput {
            bindings
                .try_reserve_exact(prepared.bindings().len())
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            bindings.extend_from_slice(prepared.bindings());
        }
        let sync = self
            .synchronization()?
            .prepare_thread_exit_with_events(
                id,
                &bindings,
                files
                    .as_ref()
                    .map_or(&[][..], |files| files.event_handles()),
            )
            .map_err(HostError::Synchronization)?;
        if let Some(receipt) = &activation {
            self.check_activation_thread_exit(receipt)?;
        }
        self.check_com_thread_exit(&com)?;
        let memory_version = self.memory.as_ref().unwrap().transaction_version();
        // exact committed ranges are unmapped before infallible kernel-object publication.
        if let Some(prepared) = dinput {
            self.dinput.as_mut().unwrap().commit_thread_exit(self.memory.as_mut().unwrap(), prepared)
                .expect("DI COM and aggregate RAM retirement were preflighted under unchanged ownership");
        } else if !ranges.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .unmap_ranges_with_word_writes(&ranges, words)
                .map_err(HostError::Memory)?;
        } else if !words.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .write_thread_words32(words)
                .map_err(HostError::Memory)?;
        }
        self.commit_automation_array_retirement(automation_arrays);
        self.commit_windows_runtime_retirement_after_memory(automation_retirement);
        self.commit_automation_temporary_retirement(&automation_allocations);
        let memory_changed = self.memory.as_ref().unwrap().transaction_version() != memory_version;
        self.synchronization
            .as_mut()
            .unwrap()
            .context_mut()
            .commit_signals_and_release(sync);
        if let Some(prepared) = user32 {
            self.user32
                .as_mut()
                .unwrap()
                .commit_thread_termination(prepared);
        }
        self.commit_windows_input_flags(input);
        self.commit_user32_window_event(window_event, old_windows);
        for (sender, continuation) in remote {
            let thread = self
                .threads
                .threads
                .iter_mut()
                .find(|thread| thread.id == sender)
                .unwrap();
            thread
                .accept_remote_result(continuation, 0)
                .expect("remote sender continuation was preflighted before thread teardown");
            if thread.suspend_count == 0 {
                thread.state = ThreadState32::Runnable;
            }
        }
        if let Some(prepared) = files {
            self.files
                .as_mut()
                .unwrap()
                .commit_thread_exit(id, prepared);
        }
        if let Some(resources) = self.resources.as_mut() {
            resources.commit_thread_exit(id);
        }
        self.threads.fls.retire_thread(id);
        self.exceptions.retire_thread(id);
        self.nls.retire_thread(id);
        self.automation_retire_thread(id);
        if let Some(gdi) = self.gdi.as_mut() {
            gdi.retire_thread(id);
            if let Some(context) = self.user32.as_mut() {
                context
                    .retire_resource_dialog_fonts(gdi)
                    .expect("dialog fonts retain their canonical GDI object identity");
            }
        }
        self.commit_activation_thread_exit(activation);
        self.commit_com_thread_exit_after_memory(com, memory_changed);
        if let Some(runtime) = self.ole32.as_mut() {
            runtime.commit_thread_exit(id);
        }
        if let Some(registry) = self.registry.as_mut() {
            registry.commit_thread_exit(id);
        }
        for range in &self.threads.threads[index].reserved {
            self.windows_memory.revoke(*range);
        }
        if id == self.threads.current_id() {
            let _ = self
                .take_windows_thread_context()
                .expect("thread exit CPU context was preflighted before RAM teardown");
        }
        let record = &mut self.threads.threads[index];
        record.state = ThreadState32::Terminated;
        record.exit_code = code;
        record.exited_filetime = exited_filetime;
        record.saved = None;
        record.active_invocations.clear();
        record.reserved.clear();
        record.teb = None;
        record.stack = None;
        record.tls_expansion = None;
        record.tls_values.clear();
        record.tls_array = None;
        record.static_tls.clear();
        record.context_override = None;
        self.threads
            .invocations
            .retain(|invocation| invocation.target != id);
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::process) fn terminate_windows_thread_for_test(
        &mut self,
        id: u32,
        code: u32,
    ) -> Result<(), HostError> {
        self.terminate_windows_thread(id, code)
    }
    pub(in crate::process) fn resolve_thread_handle(
        &self,
        handle: u32,
        access: u32,
    ) -> Result<u32, HostError> {
        if handle == u32::MAX - 1 {
            return Ok(self.threads.current_id());
        }
        self.synchronization()?
            .validate_thread_access(handle, access)
            .map_err(HostError::Synchronization)
    }
    pub(super) fn change_thread_suspend(
        &mut self,
        handle: u32,
        resume: bool,
    ) -> Result<u32, HostError> {
        let id = self.resolve_thread_handle(handle, 2)?;
        let record = self
            .threads
            .threads
            .iter_mut()
            .find(|thread| thread.id == id)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        if record.state == ThreadState32::Terminated {
            return Err(HostError::Thread(ThreadError32::AccessDenied));
        }
        let previous = record.suspend_count;
        if resume {
            record.suspend_count = record.suspend_count.saturating_sub(1);
        } else {
            record.suspend_count = record
                .suspend_count
                .checked_add(1)
                .filter(|count| *count <= 127)
                .ok_or(HostError::Thread(ThreadError32::InvalidParameter))?;
        }
        if record.suspend_count != 0 {
            record.state = ThreadState32::Suspended;
        } else if previous != 0 {
            record.state = if record
                .saved
                .as_ref()
                .is_some_and(|saved| saved.pending.is_some())
            {
                ThreadState32::Waiting
            } else {
                ThreadState32::Runnable
            };
        }
        Ok(previous)
    }
}
