use super::{RegistryError32 as E, memory, output::Outputs32};
use crate::{process::EngineInstance, windows::registry::Advapi32Api32 as Api};
impl EngineInstance {
    pub(super) fn registry_value(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        let mem = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let registry = &self.registry.as_ref().ok_or(E::NotConfigured)?.native;
        let get = matches!(api, Api::RegGetValueA | Api::RegGetValueW);
        let legacy = matches!(api, Api::RegQueryValueA | Api::RegQueryValueW);
        let (name, path, flags, type_pointer, data_pointer, size_pointer) = if get {
            (
                memory::string(mem, args[2], api.wide(), 16383)?,
                memory::string(mem, args[1], api.wide(), 32767)?,
                args[3],
                args[4],
                args[5],
                args[6],
            )
        } else if legacy {
            (
                Vec::new(),
                memory::string(mem, args[1], api.wide(), 32767)?,
                0,
                0,
                args[2],
                args[3],
            )
        } else {
            if args[2] != 0 {
                return Err(E::InvalidParameter);
            }
            (
                memory::string(mem, args[1], api.wide(), 16383)?,
                Vec::new(),
                0,
                args[3],
                args[4],
                args[5],
            )
        };
        if data_pointer != 0 && size_pointer == 0 {
            return Err(E::InvalidParameter);
        }
        if get && flags & !(0xffff | 0x30000 | 0x30000000) != 0 || get && flags & 0x30000 == 0x30000
        {
            return Err(E::InvalidParameter);
        }
        let capacity = if size_pointer == 0 {
            0
        } else {
            memory::dword(mem, size_pointer)? as usize
        };
        let value = registry.query_subkey_value(
            args[0],
            &path,
            if get { (flags & 0x30000) >> 8 } else { 0 },
            &name,
        );
        let mut outputs = Outputs32::new();
        let mut result = 0;
        let mut bytes = Vec::new();
        match value {
            Err(error) => result = error.win32(),
            Ok(value) => {
                let mut output_type = value.kind;
                if get && flags & 0xffff != 0 && flags & 0xffff != 0xffff {
                    let bit = match value.kind {
                        0 => 1,
                        1 => 2,
                        2 => 4,
                        3 => 8,
                        4 => 16,
                        7 => 32,
                        11 => 64,
                        _ => 0,
                    };
                    if flags & bit == 0 {
                        result = E::UnsupportedType.win32();
                    }
                    if result == 0
                        && matches!(flags & 0xffff, 0x18 | 0x48)
                        && value.kind == 3
                        && value.bytes.len() != if flags & 0xffff == 0x18 { 4 } else { 8 }
                    {
                        result = E::DatatypeMismatch.win32();
                    }
                }
                let mut expanded_bytes = None;
                if get && value.kind == 2 && flags & 0x10000000 == 0 && result == 0 {
                    if !value.bytes.len().is_multiple_of(2) {
                        return Err(E::InvalidData);
                    }
                    let units: Vec<u16> = value
                        .bytes
                        .chunks_exact(2)
                        .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
                        .take_while(|&unit| unit != 0)
                        .collect();
                    let source = String::from_utf16(&units).map_err(|_| E::InvalidData)?;
                    let expanded = self.expand_windows_environment(&source);
                    expanded_bytes = Some(memory::encoded_name(
                        &expanded.encode_utf16().collect::<Vec<_>>(),
                        api.wide(),
                        true,
                    )?);
                    output_type = 1;
                }
                let mut length = if let Some(bytes) = &expanded_bytes {
                    bytes.len()
                } else {
                    memory::value_guest_length(value.kind, value.bytes, api.wide())?
                };
                let mut additional = 0;
                if get && expanded_bytes.is_none() && matches!(output_type, 1 | 2 | 7) {
                    if !value.bytes.len().is_multiple_of(2) {
                        return Err(E::InvalidData);
                    }
                    let terminators: usize = if output_type == 7 { 2 } else { 1 };
                    let trailing = value
                        .bytes
                        .chunks_exact(2)
                        .rev()
                        .take_while(|bytes| *bytes == [0, 0])
                        .count();
                    additional =
                        terminators.saturating_sub(trailing) * if api.wide() { 2 } else { 1 };
                    length = length.checked_add(additional).ok_or(E::NotEnoughMemory)?;
                }
                if result == 0 && data_pointer != 0 {
                    if capacity < length {
                        result = E::MoreData.win32();
                    } else {
                        memory::output(mem, data_pointer, length)?;
                        bytes = match expanded_bytes {
                            Some(bytes) => bytes,
                            None => memory::value_to_guest(value.kind, value.bytes, api.wide())?,
                        };
                        bytes
                            .try_reserve_exact(additional)
                            .map_err(|_| E::NotEnoughMemory)?;
                        bytes.resize(length, 0);
                    }
                }
                outputs.word(mem, type_pointer, output_type)?;
                outputs.word(mem, size_pointer, length as u32)?;
            }
        }
        if result == 0 && data_pointer != 0 {
            outputs.bytes(mem, data_pointer, bytes)?;
        } else if result != 0 && get && flags & 0x20000000 != 0 && data_pointer != 0 {
            memory::output(mem, data_pointer, capacity)?;
            let mut zero = Vec::new();
            zero.try_reserve_exact(capacity)
                .map_err(|_| E::NotEnoughMemory)?;
            zero.resize(capacity, 0);
            outputs.bytes(mem, data_pointer, zero)?;
        }
        outputs.publish(self.memory.as_mut().unwrap())?;
        Ok(result)
    }
}
