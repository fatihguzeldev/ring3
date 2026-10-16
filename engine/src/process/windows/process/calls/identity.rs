use super::*;

impl EngineInstance {
    pub(super) fn execute_process_identity_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let wide = api.name().ends_with('W');
        let code_page = self.windows_ansi_code_page();
        let result = match api {
            GetCurrentProcessId => self.process_runtime.launch.process_id,
            GetCurrentThreadId => self.current_windows_thread_id()?,
            GetCurrentThread => u32::MAX - 1,
            GetStdHandle | SetStdHandle => {
                let index = match args[0] {
                    0xffff_fff6 => 0,
                    0xffff_fff5 => 1,
                    0xffff_fff4 => 2,
                    _ => {
                        return Ok(ProcessReply32 {
                            value: if api == GetStdHandle {
                                u64::from(u32::MAX)
                            } else {
                                0
                            },
                            error: Some(87),
                        });
                    }
                };
                if api == GetStdHandle {
                    self.process_standard_handles()?[index]
                } else {
                    if let Some(startup) = self.process_runtime.startup.as_ref() {
                        buffers::write(
                            self.memory.as_mut().unwrap(),
                            startup.peb + 0x118 + index as u32 * 4,
                            &args[1].to_le_bytes(),
                        )
                        .map_err(guest)?;
                    }
                    self.process_runtime.standard_handles[index] = args[1];
                    1
                }
            }
            EncodePointer => crate::windows::process::PointerCodec32::new(self.key).encode(args[0]),
            DecodePointer => crate::windows::process::PointerCodec32::new(self.key).decode(args[0]),
            IsProcessorFeaturePresent => {
                u32::from(crate::windows::process::processor_feature_present(args[0]))
            }
            GetStartupInfoA | GetStartupInfoW => {
                let mut bytes = [0; 68];
                bytes[..4].copy_from_slice(&68u32.to_le_bytes());
                for (index, handle) in self.process_standard_handles()?.iter().enumerate() {
                    bytes[56 + index * 4..60 + index * 4].copy_from_slice(&handle.to_le_bytes());
                }
                bytes[44..48].copy_from_slice(&self.process_runtime.startup_flags.to_le_bytes());
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                0
            }
            IsDebuggerPresent => 0,
            CheckRemoteDebuggerPresent => {
                if let Err(error) = self.validate_windows_process_handle(args[0], 0x400) {
                    return Ok(ProcessReply32 {
                        value: 0,
                        error: Some(error.win32()),
                    });
                }
                buffers::write(self.memory.as_mut().unwrap(), args[1], &0u32.to_le_bytes())
                    .map_err(guest)?;
                1
            }
            GetSystemInfo => {
                let words = [0, 4096, 0x10000, 0x7ffeffff, 1, 1, 586, 65536, 0x0301_0006];
                let bytes = words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                0
            }
            GetVersion => 0x0a28_0105,
            GetVersionExA | GetVersionExW => {
                let mut size = [0; 4];
                self.memory()?
                    .read(crate::memory::GuestAddress(args[0]), &mut size)
                    .map_err(|_| guest(ProcessError32::NoAccess))?;
                let size = u32::from_le_bytes(size) as usize;
                let basic = if wide { 276 } else { 148 };
                if size != basic && size != basic + 8 {
                    return Err(guest(ProcessError32::InvalidParameter));
                }
                let mut bytes = vec![0; size];
                for (index, word) in [size as u32, 5, 1, 2600, 2].into_iter().enumerate() {
                    bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
                }
                let service = buffers::encoded("Service Pack 3", wide, code_page);
                bytes[20..20 + service.len()].copy_from_slice(&service);
                if size > basic {
                    bytes[basic..basic + 2].copy_from_slice(&3u16.to_le_bytes());
                    bytes[basic + 6] = 1;
                }
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                1
            }
            _ => return Err(HostError::InvalidRequest),
        };
        Ok(result.into())
    }
}
