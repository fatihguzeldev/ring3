use super::*;

impl EngineInstance {
    pub(super) fn execute_process_modules_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let wide = api.name().ends_with('W');
        let code_page = self.windows_ansi_code_page();
        let result = match api {
            GetModuleHandleExA | GetModuleHandleExW => {
                buffers::write(self.memory.as_mut().unwrap(), args[2], &0u32.to_le_bytes())
                    .map_err(guest)?;
                if args[0] & !7 != 0 || args[0] & 3 == 3 {
                    return Err(guest(ProcessError32::InvalidParameter));
                }
                let name = if args[0] & 4 == 0 && args[1] != 0 {
                    Some(buffers::string(self.memory()?, args[1], wide, code_page).map_err(guest)?)
                } else {
                    None
                };
                let prepared = self.prepare_module_handle_reference(
                    name.as_deref(),
                    (args[0] & 4 != 0).then_some(crate::memory::GuestAddress(args[1])),
                    args[0],
                );
                let Some((handle, prepared)) = (match prepared {
                    Ok(prepared) => prepared,
                    Err(HostError::Module(crate::loader::ModuleError32::ReferenceExhausted)) => {
                        return Err(guest(ProcessError32::Capacity));
                    }
                    Err(error) => return Err(error),
                }) else {
                    return Ok(ProcessReply32 {
                        value: 0,
                        error: Some(126),
                    });
                };
                buffers::write(
                    self.memory.as_mut().unwrap(),
                    args[2],
                    &handle.to_le_bytes(),
                )
                .map_err(guest)?;
                self.commit_module_handle_reference(prepared);
                1
            }
            GetModuleFileNameA | GetModuleFileNameW => {
                if args[2] == 0 {
                    return Err(guest(ProcessError32::InvalidParameter));
                }
                let mut bytes = if wide {
                    self.virtual_module_file_name_units(args[0])?
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    buffers::encoded(&self.virtual_module_file_name(args[0])?, false, code_page)
                };
                let width = if wide { 2 } else { 1 };
                let length = bytes.len() / width;
                if length >= args[2] as usize {
                    bytes.truncate(args[2] as usize * width);
                    let tail = bytes.len() - width;
                    bytes[tail..].fill(0);
                    buffers::write(self.memory.as_mut().unwrap(), args[1], &bytes)
                        .map_err(guest)?;
                    return Ok(ProcessReply32 {
                        value: u64::from(args[2]),
                        error: Some(122),
                    });
                }
                bytes.resize(bytes.len() + width, 0);
                buffers::write(self.memory.as_mut().unwrap(), args[1], &bytes).map_err(guest)?;
                length as u32
            }
            _ => return Err(HostError::InvalidRequest),
        };
        Ok(result.into())
    }
}
