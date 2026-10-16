use super::super::{
    DirectInputRuntime32,
    memory::{output, read, words},
    objects::ObjectKind32,
};
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32, haptics::EffectKind32},
    memory::{AddressSpace, GuestAddress},
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn create_effect(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, H> {
        output(memory, args[3], 4)?;
        if args[4] != 0 {
            return Err(H::NO_AGGREGATION);
        }
        self.force_device(pointer)?;
        let kind =
            EffectKind32::from_guid(read(memory, args[1])?).ok_or(H::DEVICE_NOT_REGISTERED)?;
        let parameters = if args[2] == 0 {
            None
        } else {
            Some(self.read_effect_parameters(
                memory,
                pointer,
                kind,
                args[2],
                if words::<1>(memory, args[2])?[0] == 52 {
                    0x1ff
                } else {
                    0x3ff
                },
                None,
            )?)
        };
        let prepared = self.force_device(pointer)?.prepare_effect(
            kind,
            parameters,
            self.force_exclusive(pointer)?,
        )?;
        let canonical = self.object(pointer)?.pointer;
        let version = self.object(pointer)?.version;
        let interface = if self.object(pointer)?.interface.generation() == 8 {
            InterfaceKind32::Effect8
        } else {
            InterfaceKind32::EffectLegacy
        };
        self.publish(
            memory,
            interface,
            version,
            ObjectKind32::Effect {
                device: canonical,
                id: prepared.id(),
            },
            args[3],
            Some(canonical),
        )?;
        self.force_device_mut(pointer)
            .expect("effect device was preflighted")
            .commit_effect(prepared);
        Ok(0)
    }
}
