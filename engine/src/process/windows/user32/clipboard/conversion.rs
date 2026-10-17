use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::{
        text::CodePage32,
        user32::{User32Error32 as Error, clipboard::ClipboardContent32 as Content},
    },
};

impl EngineInstance {
    pub(super) fn resolve_clipboard_data(&mut self, format: u32) -> Result<u32, HostError> {
        let Some(content) = self
            .user32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .clipboard
            .get(format)
        else {
            return Ok(0);
        };
        if let Some(memory) = content.memory() {
            self.check_windows_global_memory(memory)?;
            return Ok(memory.handle);
        }
        let Content::Synthesized { source, .. } = content else {
            return Ok(0);
        };
        let bytes = if format == 16 {
            self.user32
                .as_ref()
                .unwrap()
                .clipboard
                .locale()
                .to_le_bytes()
                .to_vec()
        } else {
            let Some(source_memory) = self
                .user32
                .as_ref()
                .unwrap()
                .clipboard
                .get(source)
                .and_then(Content::memory)
            else {
                return Ok(0);
            };
            self.check_windows_global_memory(source_memory)?;
            let bytes = self.clipboard_text_bytes(
                source_memory.pointer,
                source_memory.bytes,
                source == 13,
            )?;
            let units = if source == 13 {
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>()
            } else {
                let code_page = self.clipboard_code_page(source == 7)?;
                code_page
                    .decode(&bytes, false)
                    .map_err(|_| HostError::User32(Error::Win32(1113)))?
            };
            if format == 13 {
                units
                    .into_iter()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                let code_page: CodePage32 = self.clipboard_code_page(format == 7)?;
                let mut bytes = code_page
                    .encode(&units, false)
                    .map_err(|_| HostError::User32(Error::Win32(1113)))?
                    .0;
                bytes.push(0);
                bytes
            }
        };
        let memory = self.allocate_windows_transferred_global(&bytes)?;
        self.user32
            .as_mut()
            .unwrap()
            .clipboard
            .commit_synthesized(format, memory);
        Ok(memory.handle)
    }

    fn clipboard_code_page(&mut self, oem: bool) -> Result<CodePage32, HostError> {
        let clipboard = &self.user32.as_ref().unwrap().clipboard;
        let locale = if let Some(memory) = clipboard.get(16).and_then(Content::memory) {
            self.check_windows_global_memory(memory)?;
            if memory.bytes < 4 {
                return Err(HostError::User32(Error::Win32(13)));
            }
            let mut bytes = [0; 4];
            self.memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(memory.pointer), &mut bytes)
                .map_err(|error| HostError::User32(Error::Memory(error)))?;
            u32::from_le_bytes(bytes)
        } else {
            clipboard.locale()
        };
        let (_, code_page) =
            self.windows_locale_information(locale, if oem { 0xb } else { 0x1004 })?;
        let code_page = code_page.ok_or(HostError::User32(Error::Win32(87)))?;
        if code_page == 0 || code_page == 1 {
            return Ok(if oem {
                self.windows_oem_code_page()
            } else {
                self.windows_ansi_code_page()
            });
        }
        CodePage32::from_id(code_page).ok_or(HostError::User32(Error::Win32(50)))
    }

    fn clipboard_text_bytes(
        &self,
        pointer: u32,
        length: u32,
        wide: bool,
    ) -> Result<Vec<u8>, HostError> {
        let step = if wide { 2 } else { 1 };
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact((length as usize).min(512))
            .map_err(|_| HostError::User32(Error::Allocation))?;
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        for offset in (0..length.saturating_sub(step - 1)).step_by(step as usize) {
            let address = pointer
                .checked_add(offset)
                .ok_or(HostError::User32(Error::Win32(87)))?;
            let mut unit = [0; 2];
            memory
                .read(GuestAddress(address), &mut unit[..step as usize])
                .map_err(|error| HostError::User32(Error::Memory(error)))?;
            if unit[..step as usize].iter().all(|&byte| byte == 0) {
                return Ok(bytes);
            }
            bytes
                .try_reserve(step as usize)
                .map_err(|_| HostError::User32(Error::Allocation))?;
            bytes.extend_from_slice(&unit[..step as usize]);
        }
        Err(HostError::User32(Error::Win32(13)))
    }
}
