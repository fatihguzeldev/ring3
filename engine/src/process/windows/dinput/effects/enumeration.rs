use super::super::{
    DirectInputRuntime32,
    memory::{bytes, output, put, read, words},
    objects::ObjectKind32,
};
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32, haptics::EffectKind32},
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn effect_information(
        &self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind32,
        args: &[u32],
    ) -> Result<u32, H> {
        self.force_device(pointer)?;
        let kind = EffectKind32::from_guid(read(memory, args[2])?).ok_or(H::OBJECT_NOT_FOUND)?;
        let data = effect_information(kind, interface.wide())?;
        if words::<1>(memory, args[1])?[0] != data.len() as u32 {
            return Err(H::INVALID_PARAM);
        }
        output(memory, args[1], data.len())?;
        memory.write(GuestAddress(args[1]), &data)?;
        Ok(0)
    }
    pub(in crate::process::windows::dinput) fn enumerate_effects(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind32,
        args: &[u32],
        created: bool,
    ) -> Result<WindowsProviderOutcome32, H> {
        self.force_device(pointer)?;
        let mut rows = Vec::new();
        let mut retained = Vec::new();
        if created {
            if args[3] != 0 {
                return Err(H::INVALID_PARAM);
            }
            let device = self.object(pointer)?.pointer;
            retained
                .try_reserve_exact(self.force_device(pointer)?.len())
                .map_err(|_| H::OUT_OF_MEMORY)?;
            rows.try_reserve_exact(self.force_device(pointer)?.len())
                .map_err(|_| H::OUT_OF_MEMORY)?;
            for record in self.records.iter().flatten() {
                if matches!(record.kind,ObjectKind32::Effect{device:owner,..} if owner==device) {
                    let mut row = bytes(4)?;
                    put(&mut row, 0, record.pointer.0);
                    rows.push(row);
                    retained.push(record.pointer);
                }
            }
        } else {
            if args[3] & !0xff != 0 || args[3] > 5 {
                return Err(H::INVALID_PARAM);
            }
            rows.try_reserve_exact(12).map_err(|_| H::OUT_OF_MEMORY)?;
            for kind in EffectKind32::ALL {
                if args[3] == 0 || args[3] == kind.family() {
                    rows.push(effect_information(kind, interface.wide())?);
                }
            }
        }
        let receipt = if retained.is_empty() {
            None
        } else {
            Some(self.com.prepare_retain_changes(memory, &retained)?)
        };
        if let Some(receipt) = &receipt {
            self.com.can_commit_retain_changes(receipt)?;
        }
        let outcome = self.start_enumeration(memory, args[1], args[2], interface, rows)?;
        if let Some(receipt) = receipt {
            self.com.commit_retain_changes(receipt);
            self.pending.last_mut().unwrap().effects = Some(retained);
            return Ok(self.enumeration_callback());
        }
        Ok(outcome)
    }
}

fn effect_information(kind: EffectKind32, wide: bool) -> Result<Vec<u8>, H> {
    let mut data = bytes(if wide { 552 } else { 292 })?;
    let length = data.len() as u32;
    put(&mut data, 0, length);
    data[4..20].copy_from_slice(&kind.guid());
    let capabilities = if kind.family() == 4 { 0xf800 } else { 0x8600 };
    put(&mut data, 20, kind.family() | capabilities);
    put(&mut data, 24, 0x3ff);
    put(&mut data, 28, 0x3df);
    if wide {
        for (index, unit) in kind.name().encode_utf16().enumerate() {
            data[32 + index * 2..34 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
    } else {
        data[32..32 + kind.name().len()].copy_from_slice(kind.name().as_bytes());
    }
    Ok(data)
}
