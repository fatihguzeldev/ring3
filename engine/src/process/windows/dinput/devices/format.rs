use super::super::{
    DirectInputRuntime32,
    memory::{read, words},
};
use crate::{
    input::dinput::{
        DirectInputResult32 as H,
        api::object_selector,
        format::{DataFormat32, FormatObject32, ObjectSelector32, ObjectType32},
    },
    memory::{AddressSpace, GuestAddress},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn set_format(
        &mut self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        address: u32,
    ) -> Result<u32, H> {
        self.device(pointer)?.native.check_unacquired()?;
        let [size, object_size, flags, data_size, count, objects] = words(memory, address)?;
        if size != 24
            || object_size != 16
            || !matches!(flags, 1 | 2)
            || count > 1024
            || count != 0 && objects == 0
        {
            return Err(H::INVALID_PARAM);
        }
        let mut parsed = Vec::new();
        parsed
            .try_reserve_exact(count as usize)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for index in 0..count {
            let at = objects
                .checked_add(index.checked_mul(16).ok_or(H::INVALID_PARAM)?)
                .ok_or(H::INVALID_PARAM)?;
            let [guid, offset, kind, object_flags] = words(memory, at)?;
            if kind & !0x80ff_ff1f != 0 || object_flags & !0x0000_0f00 != 0 {
                return Err(H::INVALID_PARAM);
            }
            let selector = if guid == 0 {
                ObjectSelector32::Any
            } else {
                object_selector(read(memory, guid)?)
            };
            let object_kind = match kind & 255 {
                0 => ObjectType32::Any,
                1..=3 => ObjectType32::Axis,
                4 | 8 | 12 => ObjectType32::Button,
                16 => ObjectType32::Pov,
                _ => return Err(H::INVALID_PARAM),
            };
            let aspect = match object_flags {
                0 | 0x100 => 0,
                0x200 => 1,
                0x300 => 2,
                0x400 => 3,
                _ => return Err(H::INVALID_PARAM),
            };
            let instance = ((kind >> 8) & 0xffff) as u16;
            parsed.push(FormatObject32 {
                offset,
                selector,
                kind: object_kind,
                instance: (instance != 0xffff).then_some(instance),
                aspect,
                optional: kind & 0x8000_0000 != 0,
            });
        }
        let format = DataFormat32::new(
            self.device(pointer)?.native.description,
            data_size,
            flags == 2,
            &parsed,
        )?;
        self.device_mut(pointer)?.native.set_format(format)?;
        Ok(0)
    }
}
