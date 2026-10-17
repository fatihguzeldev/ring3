use super::*;
use crate::memory::GuestAddress;
impl EngineInstance {
    pub(super) fn execute_process_lifecycle_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let error = |code| ProcessReply32 {
            value: 0,
            error: Some(code),
        };
        Ok(match api {
            CreateProcessA | CreateProcessW => error(50),
            SetLocalTime => error(1314),
            Beep => error(if (37..=32767).contains(&args[0]) {
                50
            } else {
                87
            }),
            OutputDebugStringA => {
                if args[0] != 0 {
                    buffers::string(
                        self.memory()?,
                        args[0],
                        false,
                        self.windows_ansi_code_page(),
                    )
                    .map_err(guest)?;
                }
                0.into()
            }
            SetHandleCount => args[0].into(),
            GetProcessVersion => {
                if args[0] != 0 && args[0] != self.current_windows_process_id() {
                    return Ok(error(87));
                }
                let Some(image) = self.image else {
                    return Ok(error(87));
                };
                let mut offset = [0; 4];
                self.memory()?
                    .read(GuestAddress(image.image_base + 0x3c), &mut offset)
                    .map_err(HostError::Memory)?;
                let at = image
                    .image_base
                    .checked_add(u32::from_le_bytes(offset))
                    .and_then(|value| value.checked_add(24 + 48))
                    .ok_or(HostError::InvalidRequest)?;
                let mut version = [0; 4];
                self.memory()?
                    .read(GuestAddress(at), &mut version)
                    .map_err(HostError::Memory)?;
                let major = u16::from_le_bytes([version[0], version[1]]) as u32;
                let minor = u16::from_le_bytes([version[2], version[3]]) as u32;
                ((major << 16) | minor).into()
            }
            GetExitCodeProcess | GetPriorityClass | SetPriorityClass => {
                let access = if api == SetPriorityClass {
                    0x200
                } else {
                    0x400
                };
                if let Err(failure) = self.validate_windows_process_handle(args[0], access) {
                    return Ok(error(failure.win32()));
                }
                match api {
                    GetExitCodeProcess => {
                        buffers::write(
                            self.memory.as_mut().unwrap(),
                            args[1],
                            &259u32.to_le_bytes(),
                        )
                        .map_err(guest)?;
                        1.into()
                    }
                    GetPriorityClass => self.process_runtime.priority_class.into(),
                    _ => {
                        if !matches!(args[1], 0x20 | 0x40 | 0x80 | 0x100 | 0x4000 | 0x8000) {
                            return Ok(error(87));
                        }
                        self.process_runtime.priority_class = args[1];
                        1.into()
                    }
                }
            }
            _ => return Err(HostError::InvalidRequest),
        })
    }
}
