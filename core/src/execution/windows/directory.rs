use super::super::Access;
use super::{GuestMemory, LoadError, MemoryError, Process32, Register32, guest, parameters};

pub(super) struct Directory {
    terminated: Vec<u8>,
}

impl Process32 {
    pub(super) fn query_directory(&mut self, arguments: &[u32]) -> Result<(), MemoryError> {
        let length = self
            .current_directory
            .query(arguments[0], arguments[1], &mut self.memory)?;
        self.cpu.set_register(Register32::Eax, length);
        Ok(())
    }
}

impl Directory {
    pub(super) fn new(path: &[u8]) -> Result<Self, LoadError> {
        let root = path.len() == 3 && path[0].is_ascii_alphabetic() && &path[1..] == b":\\";
        if !root && !parameters::valid_absolute_path(path) {
            return Err(LoadError::InvalidProcessParameters);
        }
        let mut terminated = Vec::with_capacity(path.len() + 1);
        terminated.extend_from_slice(path);
        terminated.push(0);
        Ok(Self { terminated })
    }

    pub(super) fn query(
        &self,
        capacity: u32,
        output: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let required = u32::try_from(self.terminated.len()).expect("bounded directory");
        if capacity < required {
            return Ok(required);
        }
        guest::check(memory, output, self.terminated.len(), Access::Write)?;
        memory.write(u64::from(output), &self.terminated)?;
        Ok(required - 1)
    }
}
