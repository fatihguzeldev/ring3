use super::{ResourceError32 as E, memory};
use crate::{
    loader::ResourceIdentifier32 as Identifier, memory::GuestAddress, process::EngineInstance,
    windows::resources::ResourceApi32 as Api,
};
impl EngineInstance {
    pub(in crate::process) fn resource_string_units(
        &self,
        module_handle: u32,
        id: u32,
        language: u16,
    ) -> Result<Vec<u16>, crate::process::HostError> {
        let result = (|| {
            let module = self.resource_module(module_handle)?;
            let preferred = if language == 0 {
                self.current_thread_ui_language()
            } else {
                language
            };
            let mut reader = self.resource_reader(module)?;
            let location = reader.find_languages(
                &Identifier::Id(6),
                &Identifier::Id((id >> 4) + 1),
                &[preferred, preferred & 0x3ff, 0, 0x409, 9],
                true,
            )?;
            let string = reader.string_table(location, id & 15)?;
            let address = reader.address(string.data_rva, string.units * 2)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(string.units as usize * 2)
                .map_err(|_| E::Allocation)?;
            bytes.resize(string.units as usize * 2, 0);
            self.memory
                .as_ref()
                .ok_or(E::NotConfigured)?
                .read(address, &mut bytes)?;
            let mut units = Vec::new();
            units
                .try_reserve_exact(string.units as usize)
                .map_err(|_| E::Allocation)?;
            units.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
            );
            Ok::<_, E>(units)
        })();
        result.map_err(crate::process::HostError::Resource)
    }
    pub(super) fn resource_string(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        if (args[3] as i32) < 0 || api == Api::LoadStringA && args[3] == 0 {
            return Err(E::InvalidParameter);
        }
        let module = self.resource_module(args[0])?;
        let preferred = self.current_thread_ui_language();
        let mut reader = self.resource_reader(module)?;
        let location = reader.find_languages(
            &Identifier::Id(6),
            &Identifier::Id((args[1] >> 4) + 1),
            &[preferred, preferred & 0x3ff, 0, 0x409, 9],
            true,
        )?;
        let string = reader.string_table(location, args[1] & 15)?;
        let address = reader.address(string.data_rva, string.units * 2)?.0;
        let code_page = self.windows_ansi_code_page();
        let mem = self.memory.as_mut().ok_or(E::NotConfigured)?;
        if args[3] == 0 {
            memory::output(mem, args[2], 4)?;
            mem.write(GuestAddress(args[2]), &address.to_le_bytes())?;
            return Ok(string.units);
        }
        let maximum = (args[3] - 1) as usize;
        let (bytes, copied) = if api.wide() {
            let count = (string.units as usize).min(maximum);
            memory::output(mem, args[2], (count + 1) * 2)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact((count + 1) * 2)
                .map_err(|_| E::Allocation)?;
            bytes.resize((count + 1) * 2, 0);
            mem.read(GuestAddress(address), &mut bytes[..count * 2])?;
            (bytes, count)
        } else {
            let mut source = Vec::new();
            source
                .try_reserve_exact(string.units as usize * 2)
                .map_err(|_| E::Allocation)?;
            source.resize(string.units as usize * 2, 0);
            mem.read(GuestAddress(address), &mut source)?;
            let units = source
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>();
            let mut bytes = code_page
                .encode(&units, false)
                .map_err(|_| E::InvalidParameter)?
                .0;
            bytes.truncate(maximum);
            let copied = bytes.len();
            memory::output(mem, args[2], copied + 1)?;
            bytes.try_reserve(1).map_err(|_| E::Allocation)?;
            bytes.push(0);
            (bytes, copied)
        };
        mem.write(GuestAddress(args[2]), &bytes)?;
        Ok(copied as u32)
    }
}
