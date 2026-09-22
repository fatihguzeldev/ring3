use std::collections::BTreeMap;

use super::{Access, Call, DispatchError, GuestMemory, Key, MemoryError, guest, read_name};

const MAX_VALUE_BYTES: usize = 64 * 1024;
const MAX_VALUES: usize = 4096;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

struct Value {
    kind: u32,
    data: Vec<u8>,
}

#[derive(Default)]
pub(super) struct Values {
    entries: BTreeMap<(Key, String), Value>,
    bytes: usize,
}

impl Values {
    pub(super) fn set_default(
        &mut self,
        key: Key,
        source: u32,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        for offset in 0..u32::try_from(MAX_VALUE_BYTES).expect("bounded value length") {
            let address = source
                .checked_add(offset)
                .ok_or(MemoryError::AddressOverflow)?;
            let mut byte = [0];
            memory.read(u64::from(address), &mut byte)?;
            if byte[0] == 0 {
                return self.set(
                    (key, String::new()),
                    &[0, 0, 0, 1, source, offset + 1],
                    memory,
                );
            }
        }
        Err(DispatchError::Unsupported)
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        key: Key,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if args[2] != 0 {
            return Err(DispatchError::Unsupported);
        }
        let name = read_name(memory, args[1])?;
        let identity = (key, name);
        if matches!(call, Call::Set) {
            return self.set(identity, args, memory);
        }
        if args[4] != 0 && args[5] == 0 {
            return Err(DispatchError::Unsupported);
        }
        let Some(value) = self.entries.get(&identity) else {
            return Ok(2);
        };
        query(value, args, memory)
    }

    fn set(
        &mut self,
        identity: (Key, String),
        args: &[u32],
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        let length = args[5] as usize;
        if !matches!(args[3], 0..=5 | 7 | 11) || (args[4] == 0 && length != 0) {
            return Err(DispatchError::Unsupported);
        }
        let previous = self.entries.get(&identity);
        let bytes = self.bytes - previous.map_or(0, |value| value.data.len());
        if length > MAX_VALUE_BYTES
            || bytes + length > MAX_TOTAL_BYTES
            || (previous.is_none() && self.entries.len() == MAX_VALUES)
        {
            return Ok(8);
        }
        guest::check(memory, args[4], length, Access::Read)?;
        let mut data = vec![0; length];
        memory.read(u64::from(args[4]), &mut data)?;
        validate(args[3], &data)?;
        self.entries.insert(
            identity,
            Value {
                kind: args[3],
                data,
            },
        );
        self.bytes = bytes + length;
        Ok(0)
    }
}

fn validate(kind: u32, data: &[u8]) -> Result<(), DispatchError> {
    let valid = match kind {
        0 | 3 => true,
        4 | 5 => data.len() == 4,
        11 => data.len() == 8,
        1 | 2 => data.is_ascii() && data.last() == Some(&0),
        7 => data.is_ascii() && (data == [0] || data.ends_with(&[0, 0])),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(DispatchError::Unsupported)
    }
}

fn query(value: &Value, args: &[u32], memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    let mut capacity = [0];
    if args[4] != 0 {
        guest::read_words(memory, args[5], &mut capacity)?;
    }
    let length = u32::try_from(value.data.len()).expect("bounded value length");
    let short = args[4] != 0 && capacity[0] < length;
    let copy = args[4] != 0 && !short;
    if copy {
        guest::check(memory, args[4], value.data.len(), Access::Write)?;
    }
    for output in [args[3], args[5]] {
        if output != 0 {
            guest::check(memory, output, 4, Access::Write)?;
        }
    }
    if copy {
        memory.write(u64::from(args[4]), &value.data)?;
    }
    if args[3] != 0 {
        guest::write_word(memory, args[3], value.kind)?;
    }
    if args[5] != 0 {
        guest::write_word(memory, args[5], length)?;
    }
    Ok(if short { 234 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    fn identity(index: usize) -> (Key, String) {
        (
            Key {
                root: 0x8000_0001,
                path: String::new(),
            },
            index.to_string(),
        )
    }

    #[test]
    fn count_limit_allows_replacement_but_rejects_new_values() {
        let memory = GuestMemory::new(0);
        let mut values = Values::default();
        let args = [0, 0, 0, 3, 0, 0];
        for index in 0..MAX_VALUES {
            assert!(matches!(values.set(identity(index), &args, &memory), Ok(0)));
        }
        assert!(matches!(
            values.set(identity(MAX_VALUES), &args, &memory),
            Ok(8)
        ));
        assert!(matches!(values.set(identity(0), &args, &memory), Ok(0)));
        assert_eq!(values.entries.len(), MAX_VALUES);
        assert_eq!(values.bytes, 0);
    }

    #[test]
    fn data_budget_accounts_for_replaced_bytes_without_partial_values() {
        let mut memory = GuestMemory::new(16);
        memory
            .map_zeroed(0x1000, MAX_VALUE_BYTES as u64, Permissions::READ_WRITE)
            .unwrap();
        let mut values = Values::default();
        let args = [0, 0, 0, 3, 0x1000, 65536];
        for index in 0..256 {
            assert!(matches!(values.set(identity(index), &args, &memory), Ok(0)));
        }
        assert_eq!(values.bytes, MAX_TOTAL_BYTES);
        assert!(matches!(
            values.set(identity(256), &[0, 0, 0, 3, u32::MAX, 1], &memory),
            Ok(8)
        ));
        assert_eq!(values.entries.len(), 256);
        assert!(matches!(
            values.set(identity(0), &[0, 0, 0, 3, 0, 0], &memory),
            Ok(0)
        ));
        assert_eq!(values.bytes, MAX_TOTAL_BYTES - MAX_VALUE_BYTES);
        assert!(matches!(values.set(identity(256), &args, &memory), Ok(0)));
        assert_eq!(values.bytes, MAX_TOTAL_BYTES);
        assert_eq!(values.entries[&identity(0)].data.len(), 0);
    }
}
