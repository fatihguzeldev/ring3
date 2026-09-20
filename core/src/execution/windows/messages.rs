use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, MemoryError, thread};

#[derive(Default)]
pub(super) struct Messages {
    names: BTreeMap<String, u32>,
}

impl Messages {
    pub(super) fn register(
        &mut self,
        pointer: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let name = read_name(memory, pointer)?;
        if let Some(&identifier) = self.names.get(&name) {
            return Ok(identifier);
        }
        if self.names.len() == 0x4000 {
            thread::set_last_error(memory, 8)?;
            return Ok(0);
        }
        let identifier = 0xc000 + u32::try_from(self.names.len()).expect("bounded message table");
        self.names.insert(name, identifier);
        Ok(identifier)
    }
}

fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    // integer atom pointers require a separate contract from string names.
    if pointer <= 0xffff {
        return Err(DispatchError::Unsupported);
    }
    let mut name = String::new();
    for offset in 0..=255 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return if name.is_empty() {
                Err(DispatchError::Unsupported)
            } else {
                Ok(name)
            };
        }
        if offset == 255 || !byte[0].is_ascii() || (offset == 0 && byte[0] == b'#') {
            return Err(DispatchError::Unsupported);
        }
        name.push(char::from(byte[0].to_ascii_lowercase()));
    }
    unreachable!()
}
