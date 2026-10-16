use super::{RegistryError32 as E, memory, output::Outputs32};
use crate::{process::EngineInstance, windows::registry::Advapi32Api32 as Api};
impl EngineInstance {
    pub(super) fn registry_multiple(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        const LIMIT: usize = 1024 * 1024;
        let cp = self.windows_ansi_code_page();
        let mem = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let registry = &self.registry.as_ref().ok_or(E::NotConfigured)?.native;
        if args[2] == 0 || args[4] == 0 {
            return Err(E::InvalidParameter);
        }
        let length = (args[2] as usize)
            .checked_mul(16)
            .filter(|&length| length <= LIMIT)
            .ok_or(E::TransferTooLong)?;
        let mut entries = memory::input(mem, args[1], length, LIMIT)?;
        let capacity = memory::dword(mem, args[4])? as usize;
        if args[3] == 0 && capacity != 0 {
            return Err(E::InvalidParameter);
        }
        let mut data = Vec::new();
        for entry in entries.chunks_exact_mut(16) {
            let name = memory::string(
                mem,
                u32::from_le_bytes(entry[..4].try_into().unwrap()),
                api.wide(),
                16383,
                cp,
            )?;
            let value = registry.query_value(args[0], &name)?;
            let value_length = memory::value_guest_length(value.kind, value.bytes, api.wide(), cp)?;
            let offset = data
                .len()
                .checked_add(3)
                .map(|length| length & !3)
                .ok_or(E::TransferTooLong)?;
            let end = offset
                .checked_add(value_length)
                .filter(|&end| end <= LIMIT - length)
                .ok_or(E::TransferTooLong)?;
            let bytes = memory::value_to_guest(value.kind, value.bytes, api.wide(), cp)?;
            data.try_reserve(end - data.len())
                .map_err(|_| E::NotEnoughMemory)?;
            data.resize(offset, 0);
            data.extend_from_slice(&bytes);
            entry[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            entry[8..12].copy_from_slice(
                &args[3]
                    .checked_add(offset as u32)
                    .ok_or(E::NoAccess)?
                    .to_le_bytes(),
            );
            entry[12..16].copy_from_slice(&value.kind.to_le_bytes());
        }
        let mut outputs = Outputs32::new();
        outputs.word(mem, args[4], data.len() as u32)?;
        let result = if args[3] == 0 || capacity < data.len() {
            E::MoreData.win32()
        } else {
            outputs.bytes(mem, args[1], entries)?;
            outputs.bytes(mem, args[3], data)?;
            0
        };
        outputs.publish(self.memory.as_mut().unwrap())?;
        Ok(result)
    }
}
