use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, Process32, Register32, guest, thread};

pub(super) struct Environment {
    entries: Vec<Box<[u8]>>,
}

impl Environment {
    // parameters::prepare has already bounded and validated these entries.
    pub(super) fn new(entries: &[&[u8]]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|entry| {
                    let mut terminated = entry.to_vec();
                    terminated.push(0);
                    terminated.into_boxed_slice()
                })
                .collect(),
        }
    }

    fn query(
        &self,
        memory: &mut GuestMemory,
        source: u32,
        output: u32,
        capacity: u32,
    ) -> Result<u32, DispatchError> {
        let name = read_name(memory, source)?;
        let value = self.entries.iter().find_map(|entry| {
            let separator = entry.iter().position(|&byte| byte == b'=')?;
            (!name.is_empty() && entry[..separator].eq_ignore_ascii_case(&name))
                .then_some(&entry[separator + 1..])
        });
        let Some(value) = value else {
            thread::set_last_error(memory, 203)?;
            return Ok(0);
        };
        if value.len() > 32767 {
            return Err(DispatchError::Unsupported);
        }
        let required = u32::try_from(value.len()).expect("bounded environment value");
        if capacity < required {
            return Ok(required);
        }
        guest::check(memory, output, value.len(), Access::Write)?;
        memory.write(u64::from(output), value)?;
        Ok(required - 1)
    }
}

impl Process32 {
    pub(super) fn environment_query(&mut self, arguments: &[u32]) -> Result<(), DispatchError> {
        let result =
            self.environment
                .query(&mut self.memory, arguments[0], arguments[1], arguments[2])?;
        self.cpu.set_register(Register32::Eax, result);
        Ok(())
    }
}

fn read_name(memory: &GuestMemory, source: u32) -> Result<Vec<u8>, DispatchError> {
    let mut name = Vec::new();
    for offset in 0..32768 {
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(name);
        }
        if !byte[0].is_ascii() || byte[0] == b'=' {
            return Err(DispatchError::Unsupported);
        }
        name.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}
