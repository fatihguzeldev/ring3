use super::{ActivationContexts32, Frame32};
use crate::windows::activation::ActivationError32 as E;

#[derive(Clone, Copy)]
pub struct PreparedActivationPush32 {
    revision: u64,
    frame: Frame32,
}
impl PreparedActivationPush32 {
    pub fn cookie(&self) -> u32 {
        self.frame.cookie
    }
}
#[derive(Clone, Copy)]
pub struct PreparedActivationPop32 {
    revision: u64,
    thread: u32,
    cookie: u32,
}
impl ActivationContexts32 {
    pub fn current(&self, thread: u32) -> u32 {
        self.frames
            .iter()
            .rev()
            .find(|frame| frame.thread == thread)
            .map_or(0, |frame| frame.handle)
    }
    pub fn effective_lookup_handles(&self, thread: u32) -> [u32; 2] {
        let current = self.current(thread);
        [
            current,
            if self.default == current {
                0
            } else {
                self.default
            },
        ]
    }
    pub fn process_default(&self) -> u32 {
        self.default
    }
    pub fn prepare_activate(
        &self,
        thread: u32,
        handle: u32,
    ) -> Result<PreparedActivationPush32, E> {
        if thread == 0 {
            return Err(E::InvalidParameter);
        }
        if self.frames.len() >= self.limits.frames as usize {
            return Err(E::NotEnoughMemory);
        }
        if !self.frames.iter().any(|frame| frame.thread == thread) {
            let distinct = self
                .frames
                .iter()
                .enumerate()
                .filter(|(index, frame)| {
                    !self.frames[..*index]
                        .iter()
                        .any(|earlier| earlier.thread == frame.thread)
                })
                .count();
            if distinct >= self.limits.threads as usize {
                return Err(E::NotEnoughMemory);
            }
        }
        if handle != 0 {
            self.graph(handle)?;
            self.context(handle)?
                .references
                .checked_add(1)
                .ok_or(E::NotEnoughMemory)?;
        }
        self.next_revision()?;
        self.next_cookie.checked_add(1).ok_or(E::NotEnoughMemory)?;
        Ok(PreparedActivationPush32 {
            revision: self.revision,
            frame: Frame32 {
                thread,
                cookie: self.next_cookie,
                handle,
            },
        })
    }
    pub fn can_commit_activate(&self, prepared: &PreparedActivationPush32) -> Result<(), E> {
        if self.revision != prepared.revision {
            return Err(E::Stale);
        }
        Ok(())
    }
    pub fn commit_activate(&mut self, prepared: PreparedActivationPush32) -> u32 {
        self.can_commit_activate(&prepared)
            .expect("activation push was preflighted");
        if prepared.frame.handle != 0 {
            let context = self.context_mut(prepared.frame.handle).unwrap();
            context.references += 1;
            context.holds += 1;
        }
        self.frames.push(prepared.frame);
        self.next_cookie += 1;
        self.revision += 1;
        prepared.frame.cookie
    }
    pub fn prepare_deactivate(
        &self,
        thread: u32,
        flags: u32,
        cookie: u32,
    ) -> Result<PreparedActivationPop32, E> {
        if flags & !1 != 0 {
            return Err(E::InvalidParameter);
        }
        let position = self
            .frames
            .iter()
            .rposition(|frame| frame.thread == thread && frame.cookie == cookie)
            .ok_or(E::InvalidDeactivation)?;
        let top = !self.frames[position + 1..]
            .iter()
            .any(|frame| frame.thread == thread);
        if flags == 0 && !top {
            return Err(E::EarlyDeactivation);
        }
        if flags == 1 && top {
            return Err(E::InvalidParameter);
        }
        self.next_revision()?;
        Ok(PreparedActivationPop32 {
            revision: self.revision,
            thread,
            cookie,
        })
    }
    pub fn can_commit_deactivate(&self, prepared: &PreparedActivationPop32) -> Result<(), E> {
        if prepared.revision != self.revision {
            return Err(E::Stale);
        }
        Ok(())
    }
    pub fn disposed_after_deactivate(
        &self,
        prepared: &PreparedActivationPop32,
    ) -> Result<Vec<u32>, E> {
        self.can_commit_deactivate(prepared)?;
        let position = self
            .frames
            .iter()
            .rposition(|frame| frame.thread == prepared.thread && frame.cookie == prepared.cookie)
            .ok_or(E::Stale)?;
        let mut disposed = Vec::new();
        for context in self.contexts.iter().flatten() {
            let removed = self.frames[position..]
                .iter()
                .filter(|frame| frame.thread == prepared.thread && frame.handle == context.handle)
                .count();
            if removed == context.references as usize {
                disposed.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                disposed.push(context.handle);
            }
        }
        Ok(disposed)
    }
    pub fn commit_deactivate(&mut self, prepared: PreparedActivationPop32) {
        self.can_commit_deactivate(&prepared)
            .expect("activation pop was preflighted");
        let position = self
            .frames
            .iter()
            .rposition(|frame| frame.thread == prepared.thread && frame.cookie == prepared.cookie)
            .unwrap();
        for index in (position..self.frames.len()).rev() {
            if self.frames[index].thread == prepared.thread {
                let frame = self.frames.remove(index);
                self.release_frame(frame.handle);
            }
        }
        self.revision += 1;
    }
    pub fn prepare_inherit(
        &self,
        parent: u32,
        child: u32,
    ) -> Result<Option<PreparedActivationPush32>, E> {
        let handle = self.current(parent);
        if handle == 0 || self.graph(handle)?.no_inherit() {
            return Ok(None);
        }
        self.prepare_activate(child, handle).map(Some)
    }
    pub fn check_retire_thread(&self, thread: u32) -> Result<(), E> {
        if self.frames.iter().any(|frame| frame.thread == thread) {
            self.next_revision()?;
        }
        Ok(())
    }
    pub fn disposed_after_thread_exit(&self, thread: u32) -> Result<Vec<u32>, E> {
        self.check_retire_thread(thread)?;
        let mut disposed = Vec::new();
        for context in self.contexts.iter().flatten() {
            let removed = self
                .frames
                .iter()
                .filter(|frame| frame.thread == thread && frame.handle == context.handle)
                .count();
            if removed == context.references as usize {
                disposed.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                disposed.push(context.handle);
            }
        }
        Ok(disposed)
    }
    pub fn retire_thread(&mut self, thread: u32) {
        self.check_retire_thread(thread)
            .expect("activation thread retirement was preflighted");
        let mut changed = false;
        for index in (0..self.frames.len()).rev() {
            if self.frames[index].thread == thread {
                let frame = self.frames.remove(index);
                self.release_frame(frame.handle);
                changed = true;
            }
        }
        self.revision += u64::from(changed);
    }
    pub(super) fn release_frame(&mut self, handle: u32) {
        if handle == 0 {
            return;
        }
        let slot = self
            .contexts
            .iter()
            .position(|slot| {
                slot.as_ref()
                    .is_some_and(|context| context.handle == handle)
            })
            .unwrap();
        let context = self.contexts[slot].as_mut().unwrap();
        context.references -= 1;
        context.holds -= 1;
        if context.references == 0 {
            self.contexts[slot] = None;
        }
    }
}
