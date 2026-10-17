use super::*;
use crate::{
    files::FileIo32,
    loader::ResourceIdentifier32 as Id,
    process::{CallError, EngineInstance, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        comctl32::{COMMON_PROC_A, COMMON_PROC_W},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::user32::common) fn begin_common_animation_open(
        &mut self,
        args: &[u32],
        call: User32CallContext32,
        wide: bool,
    ) -> Result<Option<User32Outcome32>, HostError> {
        if !matches!(args[1], 0x464 | 0x467) {
            return Ok(None);
        }
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        let window = context
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        if !matches!(context.state.common_state(window), Ok(State::Animate(_))) {
            return Ok(None);
        }
        context.check_animation_call().map_err(HostError::User32)?;
        if context
            .animation(window)
            .map_err(HostError::User32)?
            .loading_owner
            .is_some()
        {
            return Err(HostError::Call(CallError::Busy));
        }
        if args[3] == 0 {
            let mut prepared = context
                .publish_animation(window, None, call)
                .map_err(HostError::User32)?;
            prepared.result = 0;
            return context
                .start(Continuation32::Animation(prepared))
                .map(Some)
                .map_err(HostError::User32);
        }
        let instance = if args[2] != 0 {
            args[2]
        } else {
            context
                .state
                .record(window)
                .map_err(HostError::User32)?
                .instance
        };
        let limit = self
            .gdi
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .config()
            .bitmap_bytes;
        let name = self.user32_resource_identifier(args[3], args[1] == 0x467)?;
        let payload =
            self.resource_data_payload(instance, &Id::Name(vec![65, 86, 73]), &name, 0, limit);
        match payload {
            Ok(payload) => {
                let clip = match AviClip32::parse(payload.bytes, limit) {
                    Ok(clip) => clip,
                    Err(error) => {
                        return Err(HostError::User32(E::Win32(error_code(error))));
                    }
                };
                let context = self.user32.as_mut().unwrap();
                let prepared = context
                    .publish_animation(window, Some(clip), call)
                    .map_err(HostError::User32)?;
                return context
                    .start(Continuation32::Animation(prepared))
                    .map(Some)
                    .map_err(HostError::User32);
            }
            Err(HostError::Resource(error))
                if matches!(&name, Id::Name(_))
                    && matches!(error.win32(), 1812 | 1813 | 1814 | 1815) => {}
            Err(HostError::Resource(
                crate::process::windows::resources::ResourceError32::Memory(error)
                | crate::process::windows::resources::ResourceError32::Pe(
                    crate::loader::ResourceError32::Memory(error),
                ),
            )) => return Err(HostError::Memory(error)),
            Err(HostError::Resource(error)) => {
                return Err(HostError::User32(E::Win32(error.win32())));
            }
            Err(error) => return Err(error),
        }
        let Id::Name(path) = name else { unreachable!() };
        let lease = self
            .open_provider_file(&path, false)
            .map_err(|error| match error {
                HostError::File(error) => HostError::User32(E::Win32(error.win32())),
                error => error,
            })?;
        let length = self
            .provider_file_size(lease)
            .expect("new animation source lease is current");
        let version = self
            .provider_file_version(lease)
            .expect("new animation file version is current");
        let prepared = (|| {
            let length = usize::try_from(length).map_err(|_| HostError::User32(E::Capacity))?;
            if length > limit {
                return Err(HostError::User32(E::Capacity));
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| HostError::User32(E::Allocation))?;
            self.user32
                .as_mut()
                .unwrap()
                .animation_mut(window)
                .map_err(HostError::User32)?
                .loading = bytes;
            let context = self.user32.as_mut().unwrap();
            let outcome = context
                .begin_animation_call(window, call, Some(lease), length, 1)
                .map_err(HostError::User32)?;
            let User32Outcome32::Wait { continuation } = outcome else {
                unreachable!()
            };
            context
                .animation_mut(window)
                .expect("same animation load owner")
                .loading_owner = Some(continuation);
            let mut pending = context.animation_wait(continuation).unwrap();
            pending.source_version = Some(version);
            pending.file_id = if wide { COMMON_PROC_W } else { COMMON_PROC_A };
            context.publish_animation_call(continuation, pending);
            Ok(outcome)
        })();
        match prepared {
            Ok(outcome) => Ok(Some(outcome)),
            Err(error) => {
                self.close_provider_file(lease)?;
                Err(error)
            }
        }
    }
    pub(in crate::process) fn finish_animation_wait(
        &mut self,
        token: u64,
    ) -> Result<bool, HostError> {
        let Some(mut call) = self
            .user32
            .as_ref()
            .and_then(|context| context.animation_wait(token))
        else {
            return Ok(false);
        };
        let Some(source) = call.source else {
            return Ok(false);
        };
        let (owner, outer, id, frame) = self.animation_file_owner(call)?;
        self.files
            .as_ref()
            .ok_or(HostError::Infrastructure)?
            .check_gdi_file_call(owner, outer, id, frame)?;
        if self
            .user32
            .as_ref()
            .unwrap()
            .windows()
            .get(call.window)
            .is_err()
        {
            self.check_close_provider_files(&[source])?;
            self.commit_close_provider_files(&[source]);
            if self.pending_file_call().is_some() {
                self.files.as_mut().unwrap().finish_gdi_file_call();
            }
            call.source = None;
            call.ready = true;
            call.bytes_wait = false;
            call.result = 0;
            let context = self.user32.as_mut().unwrap();
            context.last_error = Some(1400);
            context.publish_animation_call(token, call);
            return Ok(true);
        }
        if call.bytes_wait
            && !self
                .files
                .as_ref()
                .unwrap()
                .gdi_file_request_ready(owner, outer)?
        {
            return Ok(true);
        }
        if self
            .user32
            .as_ref()
            .unwrap()
            .animation(call.window)
            .map_err(HostError::User32)?
            .loading_owner
            != Some(token)
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        self.check_close_provider_files(&[source])?;
        let queue = self.prepare_user32_queue_change_for(call.call.thread_id)?;
        let result = self.poll_animation_source(call, source);
        let bytes = match result {
            Ok(FileIo32::NeedBytes(request)) => {
                self.files
                    .as_mut()
                    .unwrap()
                    .park_gdi_file_call(owner, outer, id, frame, request);
                call.bytes_wait = true;
                self.user32
                    .as_mut()
                    .unwrap()
                    .publish_animation_call(token, call);
                return Ok(true);
            }
            Ok(FileIo32::Ready(bytes)) => Ok(bytes),
            Err(HostError::File(error)) => Err(error.win32()),
            Err(HostError::User32(E::Win32(error))) => Err(error),
            Err(error) => return Err(error),
        };
        let result = bytes.and_then(|bytes| {
            AviClip32::parse(bytes, self.gdi.as_ref().unwrap().config().bitmap_bytes)
                .map_err(error_code)
        });
        let prepared = match result {
            Ok(clip) => self
                .user32
                .as_mut()
                .unwrap()
                .publish_animation(call.window, Some(clip), call.call)
                .map_err(error_code),
            Err(error) => Err(error),
        };
        self.commit_close_provider_files(&[source]);
        if self.pending_file_call().is_some() {
            self.files.as_mut().unwrap().finish_gdi_file_call();
        }
        match prepared {
            Ok(ready) => call = ready,
            Err(error) => {
                let context = self.user32.as_mut().unwrap();
                context.last_error = Some(error);
                if let Ok(value) = context.animation_mut(call.window) {
                    value.loading.clear();
                    value.loading_owner = None;
                }
                call.source = None;
                call.ready = true;
                call.bytes_wait = false;
                call.result = 0;
            }
        }
        self.user32
            .as_mut()
            .unwrap()
            .publish_animation_call(token, call);
        self.commit_user32_queue_change(queue);
        Ok(true)
    }
    fn animation_file_owner(
        &self,
        call: AnimationCall32,
    ) -> Result<(PendingOwner, u32, u32, CallFrame32), HostError> {
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(pending.owner, pending.token)?;
        let args = pending.frame.arguments();
        if args.len() != 4 || args[0] != call.window.get() || !matches!(args[1], 0x464 | 0x467) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let id = call.file_id;
        if !matches!(id, COMMON_PROC_A | COMMON_PROC_W) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        Ok((pending.owner, pending.token, id, pending.frame))
    }
    fn poll_animation_source(
        &mut self,
        call: AnimationCall32,
        source: ProviderFile32,
    ) -> Result<FileIo32<Vec<u8>>, HostError> {
        if Some(self.provider_file_version(source)?) != call.source_version {
            return Err(HostError::File(crate::files::FileError32::FileInvalid));
        }
        let chunk = self.provider_file_read_limit()?.min(64 * 1024);
        loop {
            let received = self
                .user32
                .as_ref()
                .unwrap()
                .animation(call.window)
                .map_err(HostError::User32)?
                .loading
                .len();
            if received == call.length {
                return Ok(FileIo32::Ready(std::mem::take(
                    &mut self
                        .user32
                        .as_mut()
                        .unwrap()
                        .animation_mut(call.window)
                        .map_err(HostError::User32)?
                        .loading,
                )));
            }
            let count = (call.length - received).min(chunk);
            match self.read_provider_file(source, received as u64, count)? {
                FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                FileIo32::Ready(bytes) => {
                    if bytes.len() != count {
                        return Err(HostError::User32(E::Win32(38)));
                    }
                    self.user32
                        .as_mut()
                        .unwrap()
                        .animation_mut(call.window)
                        .map_err(HostError::User32)?
                        .loading
                        .extend_from_slice(&bytes);
                }
            }
        }
    }
    pub(in crate::process) fn resume_animation_file_wait(
        &mut self,
        owner: PendingOwner,
        outer: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(token) = self.waiting_user32_message() else {
            return Ok(false);
        };
        let Some(call) = self
            .user32
            .as_ref()
            .and_then(|context| context.animation_wait(token))
            .filter(|call| call.source.is_some())
        else {
            return Ok(false);
        };
        let actual = self.animation_file_owner(call)?;
        if (actual.0, actual.1, actual.3) != (owner, outer, frame) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        self.resume_user32_wait(self.key(), token)?;
        Ok(true)
    }
    pub(in crate::process) fn animation_file_waiting(&self) -> bool {
        self.waiting_user32_message()
            .and_then(|token| self.user32.as_ref()?.animation_wait(token))
            .is_some_and(|call| call.source.is_some())
    }
}
