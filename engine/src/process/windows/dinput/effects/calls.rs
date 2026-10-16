use super::super::{
    DirectInputRuntime32,
    memory::{output, read, word},
    objects::ObjectKind32,
};
use crate::{
    input::dinput::{DirectInputResult32 as H, haptics::EffectKind32},
    memory::{AddressSpace, GuestAddress},
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn effect_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, H> {
        let ObjectKind32::Effect { device, id } = self.object(pointer)?.kind else {
            return Err(H::INVALID_PARAM);
        };
        let kind = self.force_device(device)?.kind(id)?;
        let exclusive = self.force_exclusive(device)?;
        let now = self.now;
        match slot {
            3 => {
                if args[1] == 0 || !(0x500..=0x800).contains(&args[2]) {
                    return Err(H::INVALID_PARAM);
                }
                let requested = EffectKind32::from_guid(read(memory, args[3])?)
                    .ok_or(H::DEVICE_NOT_REGISTERED)?;
                self.force_device_mut(device)?.initialize(id, requested)?;
            }
            4 => {
                output(memory, args[1], 16)?;
                memory.write(GuestAddress(args[1]), &kind.guid())?;
            }
            5 => return self.get_effect_parameters(memory, device, id, args[1], args[2]),
            6 => {
                let previous = self.force_device(device)?.parameters(id)?;
                let next = self.read_effect_parameters(
                    memory,
                    device,
                    kind,
                    args[1],
                    args[2],
                    Some(previous),
                )?;
                return self
                    .force_device_mut(device)?
                    .set_parameters(id, next, args[2], exclusive, now);
            }
            7 => self
                .force_device_mut(device)?
                .start(id, args[1], args[2], exclusive, now)?,
            8 => self.force_device_mut(device)?.stop(id)?,
            9 => {
                let value = self.force_device(device)?.effect_status(id, now)?;
                output(memory, args[1], 4)?;
                memory.write_words32(&[word(args[1], value)])?;
            }
            10 => return self.force_device_mut(device)?.download(id, exclusive),
            11 => self.force_device_mut(device)?.unload(id)?,
            12 => return Err(H::UNSUPPORTED),
            _ => return Err(H::INVALID_PARAM),
        }
        Ok(0)
    }
}
