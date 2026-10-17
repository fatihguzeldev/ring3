use super::{ImmError32, InputComposition32, InputContext32};

#[derive(Clone, Copy)]
struct Association32 {
    window: u32,
    thread: u32,
    context: u32,
}

pub struct InputContexts32 {
    contexts: Vec<InputContext32>,
    associations: Vec<Association32>,
    next: u32,
    context_limit: usize,
    window_limit: usize,
    unit_limit: usize,
}
impl InputContexts32 {
    pub fn new(
        context_limit: usize,
        window_limit: usize,
        unit_limit: usize,
    ) -> Result<Self, ImmError32> {
        if context_limit == 0
            || window_limit == 0
            || unit_limit == 0
            || unit_limit > i32::MAX as usize / 2
        {
            return Err(ImmError32::InvalidParameter);
        }
        Ok(Self {
            contexts: Vec::new(),
            associations: Vec::new(),
            next: 0x494d_0001,
            context_limit,
            window_limit,
            unit_limit,
        })
    }
    fn create(&mut self, thread: u32, default: bool) -> Result<u32, ImmError32> {
        if thread == 0 {
            return Err(ImmError32::InvalidParameter);
        }
        if self.contexts.len() == self.context_limit {
            return Err(ImmError32::Capacity);
        }
        let next = self.next.checked_add(1).ok_or(ImmError32::Exhausted)?;
        self.contexts
            .try_reserve(1)
            .map_err(|_| ImmError32::Allocation)?;
        let context = InputContext32::new(self.next, thread, default);
        let handle = context.handle;
        self.contexts.push(context);
        self.next = next;
        Ok(handle)
    }
    pub fn create_context(&mut self, thread: u32) -> Result<u32, ImmError32> {
        self.create(thread, false)
    }
    fn default_context(&mut self, thread: u32) -> Result<u32, ImmError32> {
        if let Some(context) = self
            .contexts
            .iter()
            .find(|context| context.thread == thread && context.default)
        {
            return Ok(context.handle);
        }
        self.create(thread, true)
    }
    pub fn window_context(&mut self, window: u32, thread: u32) -> Result<u32, ImmError32> {
        if window == 0 || thread == 0 {
            return Err(ImmError32::InvalidParameter);
        }
        if let Some(association) = self.associations.iter().find(|a| a.window == window) {
            if association.thread != thread {
                return Err(ImmError32::InvalidHandle);
            }
            return Ok(association.context);
        }
        self.default_context(thread)
    }
    pub fn associated_context(&self, window: u32, thread: u32) -> Option<u32> {
        self.associations
            .iter()
            .find(|entry| entry.window == window && entry.thread == thread)
            .map(|entry| entry.context)
            .or_else(|| {
                self.contexts
                    .iter()
                    .find(|context| context.thread == thread && context.default)
                    .map(|context| context.handle)
            })
    }
    pub fn context(&self, handle: u32) -> Result<&InputContext32, ImmError32> {
        self.contexts
            .iter()
            .find(|context| context.handle == handle)
            .ok_or(ImmError32::InvalidHandle)
    }
    pub fn context_mut(
        &mut self,
        handle: u32,
        thread: u32,
    ) -> Result<&mut InputContext32, ImmError32> {
        let context = self
            .contexts
            .iter_mut()
            .find(|context| context.handle == handle)
            .ok_or(ImmError32::InvalidHandle)?;
        if context.thread != thread {
            return Err(ImmError32::WrongThread);
        }
        Ok(context)
    }
    pub fn associate(&mut self, window: u32, thread: u32, context: u32) -> Result<u32, ImmError32> {
        if window == 0 || thread == 0 {
            return Err(ImmError32::InvalidParameter);
        }
        if context != 0 && self.context(context)?.thread != thread {
            return Err(ImmError32::WrongThread);
        }
        let index = self.associations.iter().position(|a| a.window == window);
        if let Some(index) = index {
            if self.associations[index].thread != thread {
                return Err(ImmError32::InvalidHandle);
            }
        } else {
            if self.associations.len() == self.window_limit {
                return Err(ImmError32::Capacity);
            }
            self.associations
                .try_reserve(1)
                .map_err(|_| ImmError32::Allocation)?;
        }
        let previous = self.window_context(window, thread)?;
        if let Some(index) = index {
            self.associations[index].context = context;
        } else {
            self.associations.push(Association32 {
                window,
                thread,
                context,
            });
        }
        Ok(previous)
    }
    pub fn destroy_context(&mut self, handle: u32, thread: u32) -> Result<(), ImmError32> {
        let context = self.context(handle)?;
        if context.thread != thread || context.default {
            return Err(ImmError32::WrongThread);
        }
        if self.associations.iter().any(|a| a.context == handle) {
            return Err(ImmError32::Associated);
        }
        let index = self
            .contexts
            .iter()
            .position(|context| context.handle == handle)
            .unwrap();
        self.contexts.remove(index);
        Ok(())
    }
    pub fn set_composition(
        &mut self,
        handle: u32,
        thread: u32,
        composition: InputComposition32,
    ) -> Result<(), ImmError32> {
        self.check_composition(&composition)?;
        self.context_mut(handle, thread)?.composition = composition;
        Ok(())
    }
    pub(crate) fn check_composition(
        &self,
        composition: &InputComposition32,
    ) -> Result<(), ImmError32> {
        composition.check(self.unit_limit)
    }
    pub fn retire_window(&mut self, window: u32) {
        for context in &mut self.contexts {
            if context.composing_window == Some(window) {
                context.composing_window = None;
                context.composition = Default::default();
            }
        }
        self.associations
            .retain(|association| association.window != window);
    }
    pub fn retire_thread(&mut self, thread: u32) {
        self.associations
            .retain(|association| association.thread != thread);
        self.contexts.retain(|context| context.thread != thread);
    }
    pub(crate) fn reset_compositions(&mut self) {
        for context in &mut self.contexts {
            context.composing_window = None;
            context.composition = Default::default();
        }
    }
}
