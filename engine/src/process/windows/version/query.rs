use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, windows::strings::Input32},
    windows::version::{VersionApi32 as Api, VersionBlock32, VersionError32 as E},
};

impl EngineInstance {
    pub(super) fn version_query(&mut self, api: Api, args: &[u32]) -> Result<u32, E> {
        if args[0] == 0 || args[2] == 0 || args[3] == 0 {
            return Err(E::InvalidParameter);
        }
        let memory = self.memory.as_ref().unwrap();
        let mut header = [0; 2];
        memory
            .read(GuestAddress(args[0]), &mut header)
            .map_err(E::Memory)?;
        let length = u16::from_le_bytes(header) as usize;
        if length < 40 {
            return Err(E::InvalidData);
        }
        memory
            .check_span(GuestAddress(args[0]), length, crate::memory::Access::Read)
            .map_err(E::Memory)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| E::Allocation)?;
        bytes.resize(length, 0);
        memory
            .read(GuestAddress(args[0]), &mut bytes)
            .map_err(E::Memory)?;
        let path = Input32::read(
            memory,
            args[1],
            api == Api::VerQueryValueW,
            self.windows_ansi_code_page(),
            32768,
        )
        .map_err(E::from)?;
        let value = VersionBlock32::parse(&bytes)?.query(&path.units)?;
        let mut offset = value.offset;
        let mut count = value.length;
        if api == Api::VerQueryValueA && value.text {
            let start = (length + 3) & !3;
            let at = args[0].checked_add(start as u32).ok_or(E::InvalidData)?;
            let mut h = [0; 16];
            memory.read(GuestAddress(at), &mut h).map_err(E::Memory)?;
            if &h[..4] != b"R3VA" {
                return Err(E::InvalidData);
            }
            let total = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
            let entries = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
            if entries > 4096 || total > 1024 * 1024 || start + 16 + entries * 12 > total {
                return Err(E::InvalidData);
            }
            let mut found = false;
            for i in 0..entries {
                let address = at.checked_add((16 + i * 12) as u32).ok_or(E::InvalidData)?;
                let mut row = [0; 12];
                memory
                    .read(GuestAddress(address), &mut row)
                    .map_err(E::Memory)?;
                let source = u32::from_le_bytes(row[..4].try_into().unwrap()) as usize;
                if source == value.offset {
                    offset = u32::from_le_bytes(row[4..8].try_into().unwrap()) as usize;
                    count = u32::from_le_bytes(row[8..12].try_into().unwrap()) as usize;
                    if offset < start + 16 + entries * 12
                        || offset.checked_add(count).is_none_or(|end| end > total)
                    {
                        return Err(E::InvalidData);
                    }
                    memory
                        .check_span(
                            GuestAddress(args[0].checked_add(offset as u32).ok_or(E::InvalidData)?),
                            count,
                            crate::memory::Access::Read,
                        )
                        .map_err(E::Memory)?;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(E::InvalidData);
            }
        }
        let pointer = args[0].checked_add(offset as u32).ok_or(E::InvalidData)?;
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(&[
                WordWrite32 {
                    address: GuestAddress(args[2]),
                    value: pointer,
                },
                WordWrite32 {
                    address: GuestAddress(args[3]),
                    value: count as u32,
                },
            ])
            .map_err(E::Memory)?;
        Ok(1)
    }
}
