use super::{RegistryError32 as E, memory, output::Outputs32};
use crate::{process::EngineInstance, windows::registry::Advapi32Api32 as Api};

impl EngineInstance {
    pub(super) fn registry_enumeration(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        match api {
            Api::RegQueryInfoKeyA | Api::RegQueryInfoKeyW => self.registry_info(api, args),
            Api::RegEnumValueA | Api::RegEnumValueW => self.registry_enum_value(api, args),
            _ => self.registry_enum_key(api, args),
        }
    }
    fn registry_enum_key(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        let memory = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let registry = &self.registry.as_ref().ok_or(E::NotConfigured)?.native;
        let legacy = matches!(api, Api::RegEnumKeyA | Api::RegEnumKeyW);
        if args[2] == 0
            || (!legacy && (args[3] == 0 || args[4] != 0 || (args[5] != 0 && args[6] == 0)))
        {
            return Err(E::InvalidParameter);
        }
        let capacity = if legacy {
            args[3]
        } else {
            memory::dword(memory, args[3])?
        } as usize;
        let entry = registry.enumerate_key(args[0], args[1])?;
        let name = memory::encoded_name(entry.name, api.wide(), true)?;
        let unit = if api.wide() { 2 } else { 1 };
        let length = name.len() / unit - 1;
        let mut outputs = Outputs32::new();
        let mut result = 0;
        if !legacy {
            outputs.word(memory, args[3], length as u32)?;
        }
        if capacity <= length {
            result = E::MoreData.win32();
        }
        let class = if !legacy && args[6] != 0 {
            let class_capacity = memory::dword(memory, args[6])? as usize;
            let class = memory::encoded_name(entry.class, api.wide(), true)?;
            let class_length = class.len() / unit - 1;
            outputs.word(memory, args[6], class_length as u32)?;
            if args[5] != 0 && class_capacity <= class_length {
                result = E::MoreData.win32();
            }
            Some(class)
        } else {
            None
        };
        if result == 0 {
            outputs.bytes(memory, args[2], name)?;
            if let Some(class) = class {
                outputs.bytes(memory, args[5], class)?;
            }
            if !legacy {
                outputs.time(memory, args[7], entry.modified)?;
            }
        }
        outputs.publish(self.memory.as_mut().unwrap())?;
        Ok(result)
    }
    fn registry_enum_value(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        let memory = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let registry = &self.registry.as_ref().ok_or(E::NotConfigured)?.native;
        if args[2] == 0 || args[3] == 0 || args[4] != 0 || (args[6] != 0 && args[7] == 0) {
            return Err(E::InvalidParameter);
        }
        let capacity = memory::dword(memory, args[3])? as usize;
        let data_capacity = if args[7] == 0 {
            0
        } else {
            memory::dword(memory, args[7])? as usize
        };
        let entry = registry.enumerate_value(args[0], args[1])?;
        let name = memory::encoded_name(entry.name, api.wide(), true)?;
        let length = name.len() / if api.wide() { 2 } else { 1 } - 1;
        let bytes = memory::value_to_guest(entry.value.kind, entry.value.bytes, api.wide())?;
        let mut outputs = Outputs32::new();
        outputs.word(memory, args[3], length as u32)?;
        outputs.word(memory, args[5], entry.value.kind)?;
        outputs.word(memory, args[7], bytes.len() as u32)?;
        let result = if capacity <= length || (args[6] != 0 && data_capacity < bytes.len()) {
            E::MoreData.win32()
        } else {
            0
        };
        if result == 0 {
            outputs.bytes(memory, args[2], name)?;
            outputs.bytes(memory, args[6], bytes)?;
        }
        outputs.publish(self.memory.as_mut().unwrap())?;
        Ok(result)
    }
    fn registry_info(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        let memory = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let registry = &self.registry.as_ref().ok_or(E::NotConfigured)?.native;
        if args[3] != 0 || (args[1] != 0 && args[2] == 0) {
            return Err(E::InvalidParameter);
        }
        let info = registry.query_info_encoded(args[0], !api.wide())?;
        let class = memory::encoded_name(info.class, api.wide(), true)?;
        let class_length = class.len() / if api.wide() { 2 } else { 1 } - 1;
        let mut outputs = Outputs32::new();
        let mut result = 0;
        if args[2] != 0 {
            let capacity = memory::dword(memory, args[2])? as usize;
            outputs.word(memory, args[2], class_length as u32)?;
            if args[1] != 0 {
                if capacity <= class_length {
                    result = E::MoreData.win32();
                } else {
                    outputs.bytes(memory, args[1], class)?;
                }
            }
        }
        let (key_name, key_class, value_name, value_bytes) = (
            info.maximum_subkey_name,
            info.maximum_subkey_class,
            info.maximum_value_name,
            info.maximum_value_bytes,
        );
        for (pointer, value) in [
            (args[4], info.subkeys),
            (args[5], key_name),
            (args[6], key_class),
            (args[7], info.values),
            (args[8], value_name),
            (args[9], value_bytes),
            (args[10], 20),
        ] {
            outputs.word(memory, pointer, value)?;
        }
        outputs.time(memory, args[11], info.modified)?;
        outputs.publish(self.memory.as_mut().unwrap())?;
        Ok(result)
    }
}
