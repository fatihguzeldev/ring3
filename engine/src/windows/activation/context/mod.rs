mod creation;
mod lookup;
mod references;
mod scopes;
mod stack;

use super::{ActivationConfig32, ActivationError32 as E, graph::ActivationGraph32};
use std::sync::Arc;

pub struct ActivationContexts32 {
    pub(super) limits: ActivationConfig32,
    pub(super) contexts: Vec<Option<Context32>>,
    pub(super) frames: Vec<Frame32>,
    pub(super) next_handle: u32,
    pub(super) next_cookie: u32,
    pub(super) revision: u64,
    pub(super) default: u32,
}
pub(super) struct Context32 {
    handle: u32,
    references: u32,
    holds: u32,
    disabled: bool,
    graph: Arc<ActivationGraph32>,
}
#[derive(Clone, Copy)]
pub(super) struct Frame32 {
    pub(super) thread: u32,
    pub(super) cookie: u32,
    pub(super) handle: u32,
}
impl ActivationContexts32 {
    pub fn new(limits: ActivationConfig32) -> Result<Self, E> {
        limits.validate()?;
        let mut contexts = Vec::new();
        contexts
            .try_reserve_exact(limits.contexts as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        contexts.resize_with(limits.contexts as usize, || None);
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(limits.frames as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        Ok(Self {
            limits,
            contexts,
            frames,
            next_handle: 0x5800_0000,
            next_cookie: 1,
            revision: 1,
            default: 0,
        })
    }
    fn context(&self, handle: u32) -> Result<&Context32, E> {
        self.contexts
            .iter()
            .flatten()
            .find(|context| context.handle == handle)
            .ok_or(E::InvalidHandle)
    }
    fn context_mut(&mut self, handle: u32) -> Result<&mut Context32, E> {
        self.contexts
            .iter_mut()
            .flatten()
            .find(|context| context.handle == handle)
            .ok_or(E::InvalidHandle)
    }
    fn next_revision(&self) -> Result<u64, E> {
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)
    }
    pub fn graph(&self, handle: u32) -> Result<&ActivationGraph32, E> {
        let context = self.context(handle)?;
        if context.disabled {
            return Err(E::Disabled);
        }
        Ok(&context.graph)
    }
    pub fn shared_graph(&self, handle: u32) -> Result<Arc<ActivationGraph32>, E> {
        self.graph(handle)?;
        Ok(self.context(handle)?.graph.clone())
    }
    pub fn contains(&self, handle: u32) -> bool {
        self.context(handle).is_ok()
    }
    pub fn is_empty(&self) -> bool {
        self.contexts.iter().all(Option::is_none) && self.frames.is_empty() && self.default == 0
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub use creation::{PreparedActivationContext32, PreparedActivationGroup32};
pub use scopes::{
    ActivationScope32, PreparedActivationScopeExit32, PreparedActivationThreadRetirement32,
};
pub use stack::{PreparedActivationPop32, PreparedActivationPush32};
