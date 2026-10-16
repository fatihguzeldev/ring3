use crate::{
    files::FileError32,
    memory::{Access, GuestAddress, WordWrite32},
    process::{EngineInstance, HostError},
    windows::memory::MemoryError32,
};

impl EngineInstance {
    pub(super) fn execute_security_information(
        &mut self,
        named: bool,
        wide: bool,
        query: bool,
        args: &[u32],
    ) -> Result<u32, HostError> {
        let ansi = self.windows_ansi_code_page();
        let Some(files) = self.files.as_mut() else {
            return Ok(50);
        };
        files.set_security_ansi(ansi);
        if !query {
            return Ok(files
                .set_file_security_info(
                    self.memory.as_ref().unwrap(),
                    named,
                    wide,
                    args,
                    self.windows_security.token(),
                )
                .map_or_else(FileError32::win32, |()| 0));
        }
        let descriptor = match files.query_file_security_info(
            self.memory.as_ref().unwrap(),
            named,
            wide,
            args,
            self.windows_security.token(),
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => return Ok(error.win32()),
        };
        let outputs = &args[3..8];
        if outputs[4] == 0 && outputs[..4].iter().any(|pointer| *pointer != 0) {
            return Ok(87);
        }
        if outputs[4] == 0 {
            return Ok(0);
        }
        let bytes = match descriptor.to_self_relative() {
            Ok(bytes) => bytes,
            Err(error) => return Ok(error.win32()),
        };
        for pointer in outputs.iter().filter(|pointer| **pointer != 0) {
            if let Err(error) = self
                .memory()?
                .check_span(GuestAddress(*pointer), 4, Access::Write)
            {
                return match error {
                    crate::memory::MemoryError::Fault(_) => Ok(998),
                    error => Err(HostError::Memory(error)),
                };
            }
        }
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let pointer = match self.allocate_windows_runtime_initialized(&bytes) {
            Ok(pointer) => pointer,
            Err(HostError::WindowsMemory(MemoryError32::Exhausted)) => {
                return Err(HostError::Memory(
                    crate::memory::MemoryError::VersionExhausted,
                ));
            }
            Err(HostError::WindowsMemory(error)) => return Ok(error.win32()),
            Err(error) => return Err(error),
        };
        let mut words = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; 5];
        let mut count = 0;
        for (index, output) in outputs
            .iter()
            .enumerate()
            .filter(|(_, output)| **output != 0)
        {
            let value = if index == 4 {
                pointer
            } else {
                let field = [4, 8, 16, 12][index];
                let offset = u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap());
                if offset == 0 { 0 } else { pointer + offset }
            };
            words[count] = WordWrite32 {
                address: GuestAddress(*output),
                value,
            };
            count += 1;
        }
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(&words[..count])
            .expect("security output pointers and allocation version were preflighted");
        Ok(0)
    }
}
