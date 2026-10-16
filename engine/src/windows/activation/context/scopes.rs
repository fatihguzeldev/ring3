use super::ActivationContexts32;
use crate::windows::activation::ActivationError32 as E;

#[derive(Clone, Copy)]
pub struct ActivationScope32 {
    thread: u32,
    cookie: u32,
    retained: u32,
}
impl ActivationScope32 {
    pub fn new(thread: u32, cookie: u32, retained: u32) -> Self {
        Self {
            thread,
            cookie,
            retained,
        }
    }
    pub fn retained_handle(self) -> u32 {
        self.retained
    }
}

pub struct PreparedActivationScopeExit32 {
    revision: u64,
    scope: ActivationScope32,
    disposed: Vec<u32>,
}
pub struct PreparedActivationThreadRetirement32 {
    revision: u64,
    thread: u32,
    captures: Vec<u32>,
    disposed: Vec<u32>,
    changed: bool,
}
impl PreparedActivationThreadRetirement32 {
    pub fn disposed(&self) -> &[u32] {
        &self.disposed
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
impl PreparedActivationScopeExit32 {
    pub fn disposed(&self) -> &[u32] {
        &self.disposed
    }
}
impl ActivationContexts32 {
    pub fn prepare_thread_retirement(
        &self,
        thread: u32,
        captures: Vec<u32>,
    ) -> Result<PreparedActivationThreadRetirement32, E> {
        let changed = captures.iter().any(|&handle| handle != 0)
            || self.frames.iter().any(|frame| frame.thread == thread);
        if changed {
            self.next_revision()?;
        }
        for &handle in &captures {
            if handle != 0 {
                self.context(handle)?;
            }
        }
        let mut disposed = Vec::new();
        for context in self.contexts.iter().flatten() {
            let count = self
                .frames
                .iter()
                .filter(|frame| frame.thread == thread && frame.handle == context.handle)
                .count()
                + captures
                    .iter()
                    .filter(|&&handle| handle == context.handle)
                    .count();
            if count > context.holds as usize {
                return Err(E::InvalidHandle);
            }
            if count == context.references as usize {
                disposed.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                disposed.push(context.handle);
            }
        }
        Ok(PreparedActivationThreadRetirement32 {
            revision: self.revision,
            thread,
            captures,
            disposed,
            changed,
        })
    }
    pub fn commit_thread_retirement(
        &mut self,
        prepared: PreparedActivationThreadRetirement32,
    ) -> Vec<u32> {
        assert_eq!(self.revision, prepared.revision);
        for index in (0..self.frames.len()).rev() {
            if self.frames[index].thread == prepared.thread {
                let frame = self.frames.remove(index);
                self.release_frame(frame.handle);
            }
        }
        for handle in prepared.captures {
            self.release_frame(handle);
        }
        self.revision += u64::from(prepared.changed);
        prepared.disposed
    }
    pub fn prepare_scope_exit(
        &self,
        scope: ActivationScope32,
    ) -> Result<PreparedActivationScopeExit32, E> {
        let position = self
            .frames
            .iter()
            .position(|frame| frame.thread == scope.thread && frame.cookie == scope.cookie)
            .ok_or(E::InvalidDeactivation)?;
        self.next_revision()?;
        let mut disposed = Vec::new();
        for context in self.contexts.iter().flatten() {
            let count = self.frames[position..]
                .iter()
                .filter(|frame| frame.thread == scope.thread && frame.handle == context.handle)
                .count()
                + usize::from(scope.retained == context.handle);
            if count > context.holds as usize {
                return Err(E::InvalidHandle);
            }
            if count == context.references as usize {
                disposed.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                disposed.push(context.handle);
            }
        }
        if scope.retained != 0 {
            self.context(scope.retained)?;
        }
        Ok(PreparedActivationScopeExit32 {
            revision: self.revision,
            scope,
            disposed,
        })
    }
    pub fn can_commit_scope_exit(&self, prepared: &PreparedActivationScopeExit32) -> Result<(), E> {
        if self.revision != prepared.revision {
            return Err(E::Stale);
        }
        Ok(())
    }
    pub fn commit_scope_exit(&mut self, prepared: PreparedActivationScopeExit32) -> Vec<u32> {
        self.can_commit_scope_exit(&prepared)
            .expect("activation scope retirement was preflighted");
        let scope = prepared.scope;
        let position = self
            .frames
            .iter()
            .position(|frame| frame.thread == scope.thread && frame.cookie == scope.cookie)
            .unwrap();
        for index in (position..self.frames.len()).rev() {
            if self.frames[index].thread == scope.thread {
                let frame = self.frames.remove(index);
                self.release_frame(frame.handle);
            }
        }
        self.release_frame(scope.retained);
        self.revision += 1;
        prepared.disposed
    }
}
