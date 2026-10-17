mod dispatch;
use super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::user32::{
        User32Api32 as Api, User32Error32 as E,
        accelerators::{Accelerator32, AcceleratorHandle32},
    },
};
pub(super) use dispatch::Dispatch32;

impl User32Context32 {
    pub(super) fn accelerator_call(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        use Api::*;
        if matches!(api, TranslateAcceleratorA | TranslateAcceleratorW) {
            return self.translate_accelerator(api, args, ram, call);
        }
        let value = match api {
            CreateAcceleratorTableA | CreateAcceleratorTableW => {
                if args[1] == 0 || args[1] > 4096 {
                    return Err(E::Win32(87));
                }
                let size = args[1] as usize * 6;
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(size).map_err(|_| E::Allocation)?;
                bytes.resize(size, 0);
                ram.read(GuestAddress(args[0]), &mut bytes)?;
                let mut entries = Accelerator32::api(&bytes, 4096)?;
                if !api.wide() {
                    for entry in &mut entries {
                        if entry.flags & 1 == 0 {
                            let bytes = entry.key.to_le_bytes();
                            let units = call
                                .ansi
                                .decode(&bytes[..if bytes[1] == 0 { 1 } else { 2 }], false)
                                .map_err(|_| E::Win32(1113))?;
                            entry.key = *units
                                .first()
                                .filter(|_| units.len() == 1)
                                .ok_or(E::Win32(1113))?;
                        }
                    }
                }
                self.accelerators.create(entries, None)?.get()
            }
            CopyAcceleratorTableA | CopyAcceleratorTableW => {
                let entries = self
                    .accelerators
                    .entries(AcceleratorHandle32::try_from(args[0])?)?;
                if args[1] == 0 {
                    return Ok(User32Outcome32::Return(entries.len() as u32));
                }
                if (args[2] as i32) < 0 {
                    return Err(E::Win32(87));
                }
                let count = entries.len().min(args[2] as usize);
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(count * 6)
                    .map_err(|_| E::Allocation)?;
                for entry in &entries[..count] {
                    let mut entry = *entry;
                    if !api.wide() && entry.flags & 1 == 0 {
                        let encoded = call
                            .ansi
                            .encode(&[entry.key], false)
                            .map_err(|_| E::Win32(1113))?
                            .0;
                        if encoded.len() > 2 {
                            return Err(E::Win32(1113));
                        }
                        entry.key = u16::from_le_bytes([
                            encoded.first().copied().unwrap_or(0),
                            encoded.get(1).copied().unwrap_or(0),
                        ]);
                    }
                    bytes.extend_from_slice(&entry.encode());
                }
                if !bytes.is_empty() {
                    ram.write(GuestAddress(args[1]), &bytes)?;
                }
                count as u32
            }
            DestroyAcceleratorTable => u32::from(
                self.accelerators
                    .destroy(AcceleratorHandle32::try_from(args[0])?)?,
            ),
            _ => return Err(E::InvalidRequest),
        };
        Ok(User32Outcome32::Return(value))
    }
}
