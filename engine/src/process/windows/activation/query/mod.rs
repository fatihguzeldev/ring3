mod data;
use super::{ActivationRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{activation::ActivationError32 as E, text::CodePage32},
};

impl ActivationRuntime32 {
    pub(super) fn query(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        args: &[u32],
        module_handle: Option<u32>,
    ) -> Result<u32, E> {
        let flags = args[0];
        if flags & !0x8000001c != 0
            || (flags & 0x1c).count_ones() > 1
            || flags & 4 != 0 && args[1] != 0
        {
            return Err(E::InvalidParameter);
        }
        let handle = if let Some(handle) = module_handle {
            handle
        } else if flags & 4 != 0 {
            self.native.current(thread)
        } else if args[1] == 0 && args[3] != 1 {
            self.native.process_default()
        } else {
            args[1]
        };
        let graph = if handle == 0 {
            None
        } else {
            self.storage(handle)?.check(memory)?;
            Some(self.native.graph(handle)?)
        };
        let index = if args[3] == 3 {
            [u32::from_le_bytes(memory::read(memory, args[2])?), 0]
        } else if args[3] == 4 {
            let bytes = memory::read::<8>(memory, args[2])?;
            [
                u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                u32::from_le_bytes(bytes[4..].try_into().unwrap()),
            ]
        } else {
            [0, 0]
        };
        let data = data::build(graph, handle, args[3], index, args[4])?;
        let retain = args[3] == 1 && flags & 0x80000000 == 0 && handle != 0;
        if retain {
            self.native.can_add_ref(handle)?;
        }
        let result = write_query(memory, args, &data.bytes, data.written)?;
        if retain {
            self.native
                .add_ref(handle)
                .expect("queried activation reference was preflighted");
        }
        Ok(result)
    }
    pub(super) fn settings(
        &self,
        memory: &mut AddressSpace,
        thread: u32,
        args: &[u32],
    ) -> Result<u32, E> {
        if args[0] != 0 {
            return Err(E::InvalidParameter);
        }
        let handle = if args[1] == 0 {
            let current = self.native.current(thread);
            if current == 0 {
                self.native.process_default()
            } else {
                current
            }
        } else {
            args[1]
        };
        let namespace = if args[2] == 0 {
            "http://schemas.microsoft.com/SMI/2005/WindowsSettings"
                .encode_utf16()
                .collect()
        } else {
            memory::string(memory, args[2], true, CodePage32::Windows1252)?
        };
        let name = memory::string(memory, args[3], true, CodePage32::Windows1252)?;
        if handle == 0 {
            return Err(E::KeyNotFound);
        }
        self.storage(handle)?.check(memory)?;
        let setting = self
            .native
            .graph(handle)?
            .assemblies()
            .iter()
            .flat_map(|assembly| &assembly.manifest.settings)
            .find(|setting| {
                setting
                    .namespace
                    .encode_utf16()
                    .eq(namespace.iter().copied())
                    && setting.name.encode_utf16().eq(name.iter().copied())
            })
            .ok_or(E::KeyNotFound)?;
        let bytes = setting
            .value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let required = bytes.len() as u32 / 2;
        if args[6] != 0 {
            memory::output(memory, args[6], 4)?;
        }
        if args[5] < required {
            if args[6] != 0 {
                memory
                    .write(GuestAddress(args[6]), &required.to_le_bytes())
                    .map_err(|_| E::NoAccess)?;
            }
            return Err(E::Win32(122));
        }
        memory::output(memory, args[4], bytes.len())?;
        let mut spans = vec![(GuestAddress(args[4]), bytes.as_slice())];
        let count = required.to_le_bytes();
        if args[6] != 0 {
            spans.push((GuestAddress(args[6]), &count));
        }
        memory.write_spans(&spans).map_err(|_| E::NoAccess)?;
        Ok(1)
    }
}
fn write_query(
    memory: &mut AddressSpace,
    args: &[u32],
    bytes: &[u8],
    written: u32,
) -> Result<u32, E> {
    let required = bytes.len() as u32;
    if args[6] != 0 {
        memory::output(memory, args[6], 4)?;
    }
    if args[5] < required {
        if args[6] != 0 {
            memory
                .write(GuestAddress(args[6]), &required.to_le_bytes())
                .map_err(|_| E::NoAccess)?;
        }
        return Err(E::Win32(122));
    }
    memory::output(memory, args[4], bytes.len())?;
    let mut spans = vec![(GuestAddress(args[4]), bytes)];
    let written = written.to_le_bytes();
    if args[6] != 0 {
        spans.push((GuestAddress(args[6]), &written));
    }
    memory.write_spans(&spans).map_err(|_| E::NoAccess)?;
    Ok(1)
}
