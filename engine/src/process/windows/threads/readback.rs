use super::*;
use crate::{
    graphics::d3d8::commands::ReadbackId,
    process::{
        CallError, EngineInstance, HostError,
        windows::d3d8::{D3D8Error32, D3D8ReadbackOwner32},
    },
};

fn stale() -> HostError {
    HostError::D3D8(D3D8Error32::StaleReply)
}

impl EngineInstance {
    // GPU completion belongs to the captured call, including while its thread
    // is parked. Looking it up never installs that thread's CPU context.
    pub(in crate::process) fn d3d8_readback_owner(
        &self,
        id: ReadbackId,
    ) -> Result<D3D8ReadbackOwner32, HostError> {
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let mut found = None;
        if let Some((owner, token)) = self.d3d8_reply.and_then(|reply| reply.readback_owner(id)) {
            self.checked_pending_call(owner, token)?;
            found = Some(D3D8ReadbackOwner32 {
                thread: self.threads.current_id(),
                owner,
                token,
            });
        }
        for thread in &self.threads.threads {
            if thread.state == ThreadState32::Terminated {
                continue;
            }
            for saved in thread.saved.iter().chain(
                thread
                    .active_invocations
                    .iter()
                    .map(|invocation| &invocation.saved),
            ) {
                if let Some((owner, token)) = saved.d3d8_readback_owner(id)? {
                    let key = D3D8ReadbackOwner32 {
                        thread: thread.id,
                        owner,
                        token,
                    };
                    if found.replace(key).is_some() {
                        return Err(stale());
                    }
                }
            }
        }
        found.ok_or_else(stale)
    }

    pub(in crate::process) fn checked_d3d8_readback_owner(
        &self,
        owner: D3D8ReadbackOwner32,
        id: ReadbackId,
    ) -> Result<(), HostError> {
        if self.d3d8_readback_owner(id)? != owner {
            return Err(stale());
        }
        Ok(())
    }

    pub(in crate::process) fn fail_d3d8_readback_owner(
        &mut self,
        owner: D3D8ReadbackOwner32,
        id: ReadbackId,
        result: u32,
    ) -> Result<(), HostError> {
        self.checked_d3d8_readback_owner(owner, id)?;
        if owner.thread == self.threads.current_id()
            && let Some(reply) = self.d3d8_reply.as_mut()
            && reply.readback_owner(id) == Some((owner.owner, owner.token))
        {
            reply.fail_readback(id.epoch, Some(id), result);
            return Ok(());
        }
        for thread in &mut self.threads.threads {
            if thread.id != owner.thread || thread.state == ThreadState32::Terminated {
                continue;
            }
            for saved in thread.saved.iter_mut().chain(
                thread
                    .active_invocations
                    .iter_mut()
                    .map(|invocation| &mut invocation.saved),
            ) {
                if saved.fail_d3d8_readback(owner.owner, owner.token, id, result) {
                    return Ok(());
                }
            }
        }
        Err(stale())
    }

    pub(in crate::process) fn fail_d3d8_readbacks_for_epoch(&mut self, epoch: u64, result: u32) {
        if let Some(reply) = self.d3d8_reply.as_mut() {
            reply.fail_readback(epoch, None, result);
        }
        for thread in &mut self.threads.threads {
            if thread.state == ThreadState32::Terminated {
                continue;
            }
            for saved in thread.saved.iter_mut().chain(
                thread
                    .active_invocations
                    .iter_mut()
                    .map(|invocation| &mut invocation.saved),
            ) {
                saved.fail_d3d8_readbacks_for_epoch(epoch, result);
            }
        }
    }
}
