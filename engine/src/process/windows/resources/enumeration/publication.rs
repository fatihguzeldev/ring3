use super::PendingEnum32;
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance,
        windows::resources::{ResourceError32 as E, types::ResourceCallOutcome32 as Outcome},
    },
    windows::resources::ResourceApi32 as Api,
};
impl EngineInstance {
    pub(in crate::process::windows::resources) fn begin_resource_enumeration(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<Outcome, E> {
        let module = self.resource_module(args[0])?;
        let selection = self.select_resources(api, args)?;
        let thread = self
            .current_windows_thread_id()
            .map_err(|_| E::InvalidParameter)?;
        let runtime = self.resources.as_mut().ok_or(E::NotConfigured)?;
        let slot = runtime
            .pending
            .iter()
            .position(Option::is_none)
            .ok_or(E::Capacity)?;
        let continuation = runtime
            .next_continuation
            .checked_add(1)
            .ok_or(E::Capacity)?;
        let mut pending = PendingEnum32 {
            continuation,
            thread,
            module: module.id,
            base: module.handle,
            kind: selection.kind,
            resource_type: selection.resource_type,
            resource_name: selection.resource_name,
            items: selection.items,
            index: 0,
            wide: api.wide(),
            procedure: GuestAddress(selection.procedure),
            parameter: selection.parameter,
            scratch: None,
            charged: 0,
        };
        let charged = pending.metadata_bytes();
        let total = runtime
            .enum_bytes
            .checked_add(charged)
            .filter(|&bytes| bytes <= runtime.config.max_enum_bytes as usize)
            .ok_or(E::Capacity)?;
        let (request, scratch) = pending.request(
            self.memory.as_mut().ok_or(E::NotConfigured)?,
            runtime.config.scratch,
            0,
        )?;
        pending.scratch = scratch;
        pending.charged = charged;
        runtime.pending[slot] = Some(pending);
        runtime.next_continuation = continuation;
        runtime.enum_bytes = total;
        Ok(Outcome::Callback(request))
    }
}
