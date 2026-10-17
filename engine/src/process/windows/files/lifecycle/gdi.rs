use super::super::{FileReply32, FileRuntime32};
use crate::{
    files::FileReadRequest32,
    process::{CallError, HostError, calls::call::PendingOwner},
    windows::CallFrame32,
};

impl FileRuntime32 {
    pub(in crate::process) fn check_gdi_file_call(
        &self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<(), HostError> {
        if self.reply.is_some_and(|reply| {
            (reply.owner, reply.token, reply.id, reply.frame) != (owner, token, id, frame)
        }) {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }
    pub(in crate::process) fn park_gdi_file_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
        request: FileReadRequest32,
    ) {
        self.reply = Some(FileReply32 {
            owner,
            token,
            id,
            frame,
            request: Some(request),
        });
    }
    pub(in crate::process) fn finish_gdi_file_call(&mut self) {
        self.mappings.cancel_pending();
        self.reply = None;
    }
}

impl FileRuntime32 {
    pub(in crate::process) fn gdi_file_request_ready(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<bool, HostError> {
        let reply = self
            .reply
            .filter(|reply| (reply.owner, reply.token) == (owner, token))
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        Ok(reply
            .request
            .is_none_or(|request| self.vfs.request_ready(request)))
    }
}
