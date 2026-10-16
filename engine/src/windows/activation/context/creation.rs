use super::{ActivationContexts32, Context32};
use crate::windows::activation::{ActivationError32 as E, graph::ActivationGraph32};
use std::sync::Arc;

pub struct PreparedActivationContext32 {
    revision: u64,
    slot: usize,
    context: Context32,
    process_default: bool,
}
impl PreparedActivationContext32 {
    pub fn handle(&self) -> u32 {
        self.context.handle
    }
    pub fn graph(&self) -> &ActivationGraph32 {
        &self.context.graph
    }
}
impl ActivationContexts32 {
    pub fn prepare_create(
        &self,
        graph: ActivationGraph32,
        process_default: bool,
    ) -> Result<PreparedActivationContext32, E> {
        self.prepare_create_shared(Arc::new(graph), process_default)
    }
    pub fn prepare_create_shared(
        &self,
        graph: Arc<ActivationGraph32>,
        process_default: bool,
    ) -> Result<PreparedActivationContext32, E> {
        self.next_revision()?;
        if process_default && self.default != 0 {
            return Err(E::DefaultAlreadySet);
        }
        let slot = self
            .contexts
            .iter()
            .position(Option::is_none)
            .ok_or(E::NotEnoughMemory)?;
        let handle = self.next_handle.checked_add(4).ok_or(E::NotEnoughMemory)?;
        Ok(PreparedActivationContext32 {
            revision: self.revision,
            slot,
            context: Context32 {
                handle,
                references: 1 + u32::from(process_default),
                holds: u32::from(process_default),
                disabled: false,
                graph,
            },
            process_default,
        })
    }
    pub fn can_commit_create(&self, prepared: &PreparedActivationContext32) -> Result<(), E> {
        if prepared.revision != self.revision || self.contexts[prepared.slot].is_some() {
            return Err(E::Stale);
        }
        self.next_revision()?;
        Ok(())
    }
    pub fn commit_create(&mut self, prepared: PreparedActivationContext32) -> u32 {
        self.can_commit_create(&prepared)
            .expect("activation publication was preflighted");
        let handle = prepared.context.handle;
        self.contexts[prepared.slot] = Some(prepared.context);
        self.next_handle = handle;
        if prepared.process_default {
            self.default = handle;
        }
        self.revision += 1;
        handle
    }
}

pub struct PreparedActivationGroup32 {
    revision: u64,
    contexts: Vec<PreparedActivationContext32>,
}
impl PreparedActivationGroup32 {
    pub fn handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.contexts
            .iter()
            .map(PreparedActivationContext32::handle)
    }
}
impl ActivationContexts32 {
    pub fn prepare_module_group(
        &self,
        graphs: &[(Arc<ActivationGraph32>, bool)],
    ) -> Result<PreparedActivationGroup32, E> {
        self.revision
            .checked_add(graphs.len() as u64)
            .ok_or(E::NotEnoughMemory)?;
        let mut contexts = Vec::new();
        contexts
            .try_reserve_exact(graphs.len())
            .map_err(|_| E::NotEnoughMemory)?;
        let mut slots = self
            .contexts
            .iter()
            .enumerate()
            .filter(|(_, context)| context.is_none())
            .map(|(index, _)| index);
        let mut default = self.default;
        let mut handle = self.next_handle;
        for (index, (graph, process_default)) in graphs.iter().enumerate() {
            if *process_default && default != 0 {
                return Err(E::DefaultAlreadySet);
            }
            handle = handle.checked_add(4).ok_or(E::NotEnoughMemory)?;
            if *process_default {
                default = handle;
            }
            contexts.push(PreparedActivationContext32 {
                revision: self.revision + index as u64,
                slot: slots.next().ok_or(E::NotEnoughMemory)?,
                context: Context32 {
                    handle,
                    references: 1 + u32::from(*process_default),
                    holds: 1 + u32::from(*process_default),
                    disabled: false,
                    graph: graph.clone(),
                },
                process_default: *process_default,
            });
        }
        Ok(PreparedActivationGroup32 {
            revision: self.revision,
            contexts,
        })
    }
    pub fn can_commit_group(&self, prepared: &PreparedActivationGroup32) -> Result<(), E> {
        if self.revision != prepared.revision
            || prepared
                .contexts
                .iter()
                .any(|context| self.contexts[context.slot].is_some())
        {
            return Err(E::Stale);
        }
        self.revision
            .checked_add(prepared.contexts.len() as u64)
            .ok_or(E::NotEnoughMemory)?;
        Ok(())
    }
    pub fn commit_module_group(&mut self, prepared: PreparedActivationGroup32) {
        self.can_commit_group(&prepared)
            .expect("module activation group was preflighted");
        for context in prepared.contexts {
            self.commit_create(context);
        }
    }
}
