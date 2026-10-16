use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::strings::{Input32, WindowsStringError32},
    windows::text::{
        CodePage32,
        messages::{MessageArguments32, MessageError32 as E},
    },
};

pub(super) struct Arguments32<'a> {
    memory: &'a AddressSpace,
    code_page: CodePage32,
    base: u32,
}
impl<'a> Arguments32<'a> {
    pub(super) fn new(
        memory: &'a AddressSpace,
        code_page: CodePage32,
        flags: u32,
        pointer: u32,
    ) -> Result<Self, E> {
        let base = if flags & 0x2000 == 0 && pointer != 0 {
            let mut bytes = [0; 4];
            memory
                .read(GuestAddress(pointer), &mut bytes)
                .map_err(E::Memory)?;
            u32::from_le_bytes(bytes)
        } else {
            pointer
        };
        Ok(Self {
            memory,
            code_page,
            base,
        })
    }
}
pub(super) fn string_error(error: WindowsStringError32) -> E {
    match error {
        WindowsStringError32::Memory(error) => E::Memory(error),
        WindowsStringError32::Allocation => E::Allocation,
        WindowsStringError32::FilenameTooLong => E::MoreData,
        WindowsStringError32::InvalidParameter => E::InvalidParameter,
    }
}
impl MessageArguments32 for Arguments32<'_> {
    fn word(&self, index: usize) -> Result<u32, E> {
        let at = self
            .base
            .checked_add(
                u32::try_from(index.checked_mul(4).ok_or(E::InvalidParameter)?)
                    .map_err(|_| E::InvalidParameter)?,
            )
            .ok_or(E::InvalidParameter)?;
        let mut bytes = [0; 4];
        self.memory
            .read(GuestAddress(at), &mut bytes)
            .map_err(E::Memory)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn character(&self, value: u16, wide: bool) -> Result<Vec<u16>, E> {
        if wide {
            Ok(vec![value])
        } else {
            self.code_page
                .decode(&[value as u8], false)
                .map_err(|_| E::InvalidParameter)
        }
    }
    fn string(&self, pointer: u32, wide: bool, precision: Option<usize>) -> Result<Vec<u16>, E> {
        let Some(count) = precision else {
            return Input32::read(self.memory, pointer, wide, self.code_page, 65536)
                .map(|input| input.units)
                .map_err(string_error);
        };
        if !wide {
            return Input32::ansi_prefix(self.memory, pointer, self.code_page, count, 65535)
                .map(|input| input.units)
                .map_err(string_error);
        }
        let mut units = Vec::new();
        units
            .try_reserve(count.min(256))
            .map_err(|_| E::Allocation)?;
        for index in 0..count {
            let at = pointer
                .checked_add(index as u32 * 2)
                .ok_or(E::InvalidParameter)?;
            let mut bytes = [0; 2];
            self.memory
                .read(GuestAddress(at), &mut bytes)
                .map_err(E::Memory)?;
            let unit = u16::from_le_bytes(bytes);
            if unit == 0 {
                break;
            }
            units.try_reserve(1).map_err(|_| E::Allocation)?;
            units.push(unit);
        }
        Ok(units)
    }
}
