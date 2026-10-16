use super::ReplyStage32;
use crate::{
    abi::{
        callback::CallbackRecord32,
        resident_callback::{ResidentCallbackRecord32, ResidentCallbackResult32},
    },
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
};

impl EngineInstance {
    pub fn begin_windows_resident_callback(
        &mut self,
        key: u64,
        callback_id: u64,
        return_pc: u32,
        return_id: u32,
    ) -> Result<ResidentCallbackRecord32, HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let ReplyStage32::Ready(request) = reply.stage else {
            return Err(HostError::Call(CallError::Busy));
        };
        let outer_id = match reply.owner {
            PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => id,
            _ => return Err(HostError::Call(CallError::InvalidRequest)),
        };
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.provider == super::WindowsCallbackProvider32::Ole32 {
            self.check_registered_com_callback(
                reply.owner,
                reply.token,
                reply.frame,
                reply.continuation,
            )?;
        }
        self.collect_activation_captures()?;
        let capture = (reply.provider == super::WindowsCallbackProvider32::User32)
            .then(|| {
                self.user32
                    .as_ref()
                    .and_then(|context| context.callback_activation(reply.continuation))
            })
            .flatten();
        let activation = if let Some(handle) = capture {
            self.prepare_activation_entry_borrowed(handle)?
        } else {
            self.prepare_activation_entry(None)?
        };
        let record = self.begin_provider_resident_callback(
            key,
            outer_id,
            callback_id,
            reply.token,
            request.procedure.0,
            return_pc,
            return_id,
            &request.arguments[..request.count as usize],
        )?;
        let activation = self.commit_activation_entry(activation);
        self.windows_replies.last_mut().unwrap().activation = activation;
        self.windows_replies.last_mut().unwrap().stage = ReplyStage32::Resident {
            callback_id,
            token: record.token,
        };
        Ok(record)
    }

    pub fn finish_windows_resident_callback(
        &mut self,
        key: u64,
        callback_id: u64,
        token: u32,
    ) -> Result<ResidentCallbackResult32, HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if !matches!(reply.stage,ReplyStage32::Resident { callback_id:id,token:expected } if id==callback_id&&expected==token)
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        let activation = self.prepare_activation_scope_exit(reply.activation)?;
        let result = self.finish_provider_resident_callback(key, callback_id, token)?;
        self.commit_activation_scope_exit(activation);
        self.windows_replies.last_mut().unwrap().activation = None;
        self.windows_replies.last_mut().unwrap().stage = ReplyStage32::Received(result.result);
        self.resume_windows_reply()?;
        Ok(result)
    }

    pub fn begin_windows_callback(
        &mut self,
        key: u64,
        generation: u32,
        return_pc: u32,
        return_id: u32,
    ) -> Result<CallbackRecord32, HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let ReplyStage32::Ready(request) = reply.stage else {
            return Err(HostError::Call(CallError::Busy));
        };
        if reply.owner != PendingOwner::Replacement(generation) {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.provider == super::WindowsCallbackProvider32::Ole32 {
            self.check_registered_com_callback(
                reply.owner,
                reply.token,
                reply.frame,
                reply.continuation,
            )?;
        }
        self.collect_activation_captures()?;
        let capture = (reply.provider == super::WindowsCallbackProvider32::User32)
            .then(|| {
                self.user32
                    .as_ref()
                    .and_then(|context| context.callback_activation(reply.continuation))
            })
            .flatten();
        let activation = if let Some(handle) = capture {
            self.prepare_activation_entry_borrowed(handle)?
        } else {
            self.prepare_activation_entry(None)?
        };
        let record = self.begin_provider_callback(
            key,
            generation,
            reply.token,
            request.procedure.0,
            return_pc,
            return_id,
            &request.arguments[..request.count as usize],
        )?;
        let activation = self.commit_activation_entry(activation);
        self.windows_replies.last_mut().unwrap().activation = activation;
        self.windows_replies.last_mut().unwrap().stage = ReplyStage32::Replacement {
            generation,
            token: record.token,
        };
        Ok(record)
    }

    pub fn finish_windows_callback(
        &mut self,
        key: u64,
        generation: u32,
        token: u32,
    ) -> Result<CallbackRecord32, HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if !matches!(reply.stage,ReplyStage32::Replacement { generation:expected_generation,token:expected } if expected_generation==generation&&expected==token)
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        let activation = self.prepare_activation_scope_exit(reply.activation)?;
        let result = self.finish_provider_callback(key, generation, token)?;
        self.commit_activation_scope_exit(activation);
        self.windows_replies.last_mut().unwrap().activation = None;
        self.windows_replies.last_mut().unwrap().stage = ReplyStage32::Received(result.result);
        self.resume_windows_reply()?;
        Ok(result)
    }
}
