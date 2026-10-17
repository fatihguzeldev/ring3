use super::{FileFontCall32, FileFontSource32};
use crate::{
    files::FileIo32,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        gdi::{GdiApi32, GdiError32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::gdi) fn complete_gdi_file_font(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        if let Some(files) = &self.files {
            files.check_gdi_file_call(owner, token, api.id(), frame)?;
        }
        let result = self.execute_gdi_file_font(owner, token, api, frame);
        let value = match result {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(true),
            Err(error @ HostError::Memory(_)) => return Err(error),
            Err(error @ HostError::Infrastructure) => return Err(error),
            Err(error @ HostError::Call(_)) => return Err(error),
            Err(_) => {
                let source = self.check_cancel_gdi_font_file(owner, token)?;
                self.commit_cancel_gdi_font_file(source);
                0
            }
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        if let Some(files) = self.files.as_mut() {
            files.finish_gdi_file_call();
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
    fn execute_gdi_file_font(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<Option<u32>, HostError> {
        self.collect_gdi_font_file_holds()?;
        let args = frame.arguments();
        let extended = matches!(
            api,
            GdiApi32::AddFontResourceExA
                | GdiApi32::AddFontResourceExW
                | GdiApi32::RemoveFontResourceExA
                | GdiApi32::RemoveFontResourceExW
        );
        let flags = if extended { args[1] } else { 0 };
        if flags & !0x30 != 0 || extended && args[2] != 0 {
            return Err(HostError::Gdi(GdiError32::InvalidRequest));
        }
        let add = matches!(
            api,
            GdiApi32::AddFontResourceA
                | GdiApi32::AddFontResourceW
                | GdiApi32::AddFontResourceExA
                | GdiApi32::AddFontResourceExW
        );
        let wide = matches!(
            api,
            GdiApi32::AddFontResourceW
                | GdiApi32::RemoveFontResourceW
                | GdiApi32::AddFontResourceExW
                | GdiApi32::RemoveFontResourceExW
        );
        if !add {
            let path = crate::files::path::fold(&self.font_file_path(args[0], wide)?);
            let runtime = self.gdi.as_mut().ok_or(HostError::InvalidRequest)?;
            let Some(index) = runtime.font_file_sources.iter().rposition(|source| {
                source.path == path && source.flags == flags && source.references != 0
            }) else {
                return Ok(Some(0));
            };
            let source = &runtime.font_file_sources[index];
            let lease = source.lease;
            self.check_close_font_source_files(&[lease])?;
            let runtime = self.gdi.as_mut().unwrap();
            let source = &runtime.font_file_sources[index];
            if source.references > 1 {
                runtime.font_file_sources[index].references -= 1;
                return Ok(Some(1));
            }
            runtime
                .fonts
                .remove_file_font(source.source)
                .map_err(HostError::Gdi)?;
            runtime.font_file_sources[index].references = 0;
            if !runtime
                .fonts
                .source_is_live(runtime.font_file_sources[index].source)
            {
                self.commit_close_provider_files(&[lease]);
                self.gdi.as_mut().unwrap().font_file_sources.remove(index);
            }
            return Ok(Some(1));
        }
        let thread = self.current_windows_thread_id()?;
        let index = if let Some(index) = self
            .gdi
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .font_file_calls
            .iter()
            .position(|call| call.owner == owner && call.token == token)
        {
            let call = &self.gdi.as_ref().unwrap().font_file_calls[index];
            if call.frame != frame || call.api != api || call.thread != thread {
                return Err(HostError::Call(CallError::InvalidRequest));
            }
            index
        } else {
            let path = self.font_file_path(args[0], wide)?;
            let capacity = self.windows_callback_work_capacity();
            let runtime = self.gdi.as_mut().ok_or(HostError::InvalidRequest)?;
            if runtime.font_file_calls.len() >= capacity || runtime.font_file_sources.len() >= 256 {
                return Err(HostError::Gdi(GdiError32::Capacity));
            }
            runtime
                .font_file_calls
                .try_reserve(1)
                .map_err(|_| HostError::Infrastructure)?;
            runtime
                .font_file_sources
                .try_reserve(1)
                .map_err(|_| HostError::Infrastructure)?;
            let lease = self.open_font_source_file(&path)?;
            let length = self
                .provider_file_size(lease)
                .expect("new font source lease");
            let version = self
                .provider_file_version(lease)
                .expect("new font source version");
            let path = crate::files::path::fold(&path);
            let length = usize::try_from(length).unwrap_or(usize::MAX);
            let runtime = self.gdi.as_mut().unwrap();
            runtime.font_file_calls.push(FileFontCall32 {
                owner,
                token,
                frame,
                api,
                thread,
                path,
                flags,
                lease,
                version,
                length,
                bytes: Vec::new(),
            });
            runtime.font_file_calls.len() - 1
        };
        self.poll_gdi_file_font(index)
    }
    fn poll_gdi_file_font(&mut self, index: usize) -> Result<Option<u32>, HostError> {
        let (lease, version, length) = {
            let call = &self.gdi.as_ref().unwrap().font_file_calls[index];
            (call.lease, call.version, call.length)
        };
        if length < 12 || length > 16 << 20 {
            return Err(HostError::Gdi(GdiError32::Capacity));
        }
        if self.provider_file_version(lease)? != version {
            return Err(HostError::Gdi(GdiError32::InvalidRequest));
        }
        let chunk = self.provider_file_read_limit()?.min(64 << 10);
        if chunk == 0 {
            return Err(HostError::Infrastructure);
        }
        {
            let bytes = &mut self.gdi.as_mut().unwrap().font_file_calls[index].bytes;
            bytes
                .try_reserve_exact(length - bytes.len())
                .map_err(|_| HostError::Infrastructure)?;
        }
        loop {
            let received = self.gdi.as_ref().unwrap().font_file_calls[index]
                .bytes
                .len();
            if received == length {
                break;
            }
            match self.read_provider_file(lease, received as u64, (length - received).min(chunk))? {
                FileIo32::NeedBytes(request) => {
                    let call = &self.gdi.as_ref().unwrap().font_file_calls[index];
                    self.files.as_mut().unwrap().park_gdi_file_call(
                        call.owner,
                        call.token,
                        call.api.id(),
                        call.frame,
                        request,
                    );
                    return Ok(None);
                }
                FileIo32::Ready(bytes) => {
                    if bytes.len() != (length - received).min(chunk) {
                        return Err(HostError::Gdi(GdiError32::InvalidRequest));
                    }
                    self.gdi.as_mut().unwrap().font_file_calls[index]
                        .bytes
                        .extend_from_slice(&bytes);
                }
            }
        }
        self.check_close_provider_files(&[lease])?;
        if self.provider_file_version(lease)? != version {
            return Err(HostError::Gdi(GdiError32::InvalidRequest));
        }
        let runtime = self.gdi.as_mut().unwrap();
        let call = &runtime.font_file_calls[index];
        if let Some(existing) = runtime.font_file_sources.iter().position(|source| {
            source.path == call.path
                && source.flags == call.flags
                && source.version == call.version
                && source.references != 0
        }) {
            let source = &runtime.font_file_sources[existing];
            let refs = source
                .references
                .checked_add(1)
                .ok_or(HostError::Gdi(GdiError32::Exhausted))?;
            let count = source.count;
            self.commit_close_provider_files(&[lease]);
            let runtime = self.gdi.as_mut().unwrap();
            runtime.font_file_sources[existing].references = refs;
            runtime.font_file_calls.remove(index);
            return Ok(Some(count));
        }
        let prepared = runtime
            .fonts
            .prepare_file_font_source(&call.bytes, call.flags & 0x20 == 0)
            .map_err(HostError::Gdi)?;
        let source = prepared.handle();
        let count = prepared.count();
        runtime
            .fonts
            .check_font_source(&prepared)
            .map_err(HostError::Gdi)?;
        let lease = self.commit_font_source_file_hold(lease);
        let runtime = self.gdi.as_mut().unwrap();
        runtime.fonts.commit_font_source(prepared);
        let call = runtime.font_file_calls.remove(index);
        runtime.font_file_sources.push(FileFontSource32 {
            path: call.path,
            flags: call.flags,
            version: call.version,
            source,
            lease,
            references: 1,
            count,
        });
        Ok(Some(count))
    }
}
