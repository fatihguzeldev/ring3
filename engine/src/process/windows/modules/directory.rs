use super::*;
use crate::memory::GuestAddress;
use crate::process::windows::strings::{Input32, WindowsStringError32};

impl EngineInstance {
    pub(super) fn complete_module_directory_api(
        &mut self,
        api: WindowsApi32,
        frame: CallFrame32,
    ) -> Result<(), HostError> {
        let args = frame.arguments();
        if matches!(
            api,
            WindowsApi32::SetDllDirectoryA | WindowsApi32::SetDllDirectoryW
        ) {
            let directory = if args[0] == 0 {
                None
            } else {
                let wide = api == WindowsApi32::SetDllDirectoryW;
                let cp = self.windows_ansi_code_page();
                let input = Input32::read(
                    self.memory()?,
                    args[0],
                    wide,
                    cp,
                    32768
                        * if wide {
                            1
                        } else {
                            cp.maximum_char_size() as usize
                        },
                );
                match input {
                    Ok(input) if input.units.len() > 32767 => {
                        return self.complete_module_api_return(frame, 0, Some(206));
                    }
                    Ok(input) => Some(input.units),
                    Err(error) => {
                        return self.complete_module_api_return(
                            frame,
                            0,
                            Some(input_error(error)?),
                        );
                    }
                }
            };
            let prepared = Self::prepare_completed_call(frame.complete(1))?;
            self.publish_windows_thread(self.current_windows_thread()?)?;
            self.set_module_dll_directory(directory);
            self.publish_prepared_call(prepared);
            return Ok(());
        }
        let wide = api == WindowsApi32::GetDllDirectoryW;
        let width = if wide { 2 } else { 1 };
        let units = self.module_dll_directory().unwrap_or_default();
        let mut bytes = if wide {
            units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>()
        } else {
            self.windows_ansi_code_page()
                .encode_windows(units, 0, b'?')
                .map_err(|_| HostError::Module(ModuleError32::InvalidName))?
                .0
        };
        let length = u32::try_from(bytes.len() / width).map_err(|_| HostError::Infrastructure)?;
        let result = if args[0] <= length {
            length + 1
        } else {
            length
        };
        let prepared = Self::prepare_completed_call(frame.complete(result))?;
        self.publish_windows_thread(self.current_windows_thread()?)?;
        if args[0] != 0 {
            if args[1] == 0 {
                return self.complete_module_api_return(frame, 0, Some(998));
            }
            if args[0] <= length {
                bytes.clear();
            }
            bytes.resize(bytes.len() + width, 0);
            if let Err(error) = self
                .memory
                .as_mut()
                .ok_or(HostError::Closed)?
                .write(GuestAddress(args[1]), &bytes)
            {
                let code = memory_error(error)?;
                return self.complete_module_api_return(frame, 0, Some(code));
            }
        }
        self.publish_prepared_call(prepared);
        Ok(())
    }
}

fn input_error(error: WindowsStringError32) -> Result<u32, HostError> {
    Ok(match error {
        WindowsStringError32::InvalidParameter => 87,
        WindowsStringError32::FilenameTooLong => 206,
        WindowsStringError32::Allocation => 8,
        WindowsStringError32::Memory(error) => memory_error(error)?,
    })
}

fn memory_error(error: crate::memory::MemoryError) -> Result<u32, HostError> {
    match error {
        crate::memory::MemoryError::Fault(_) | crate::memory::MemoryError::InvalidRange => Ok(998),
        other => Err(HostError::Memory(other)),
    }
}
