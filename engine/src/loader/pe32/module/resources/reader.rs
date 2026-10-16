use super::ResourceError32 as E;
use crate::{
    loader::{Directory32, ParserLimits32},
    memory::{Access, AddressSpace, GuestAddress},
};
pub struct PeResources32<'a> {
    pub(super) memory: &'a AddressSpace,
    pub(super) base: u32,
    pub(super) image_size: u32,
    pub(super) directory: Directory32,
    pub(super) entries: usize,
    pub(super) strings: usize,
    datafile: Option<&'a crate::loader::DataFileModule32>,
}
impl<'a> PeResources32<'a> {
    pub fn new(
        memory: &'a AddressSpace,
        base: u32,
        image_size: u32,
        directory: Directory32,
        limits: ParserLimits32,
    ) -> Result<Self, E> {
        if base == 0
            || image_size == 0
            || u64::from(base) + u64::from(image_size) > 1u64 << 32
            || directory.size < 16
        {
            return Err(E::Malformed);
        }
        let reader = Self {
            memory,
            base,
            image_size,
            directory,
            entries: limits.metadata_entries,
            strings: limits.string_bytes,
            datafile: None,
        };
        reader.image_span(directory.rva, directory.size)?;
        Ok(reader)
    }
    pub fn from_datafile(
        memory: &'a AddressSpace,
        module: &'a crate::loader::DataFileModule32,
        limits: ParserLimits32,
    ) -> Result<Self, E> {
        module.check(memory).map_err(|_| E::Malformed)?;
        let directory = module.resource_directory().ok_or(E::TypeNotFound)?;
        if directory.size < 16
            || u64::from(directory.rva) + u64::from(directory.size) > u64::from(module.image_size())
        {
            return Err(E::Malformed);
        }
        let reader = Self {
            memory,
            base: module.base(),
            image_size: module.image_size(),
            directory,
            entries: limits.metadata_entries,
            strings: limits.string_bytes,
            datafile: Some(module),
        };
        reader.image_span(directory.rva, 16)?;
        Ok(reader)
    }
    pub fn address(&self, rva: u32, length: u32) -> Result<GuestAddress, E> {
        self.image_span(rva, length)
    }
    pub(super) fn image_span(&self, rva: u32, length: u32) -> Result<GuestAddress, E> {
        if u64::from(rva) + u64::from(length) > u64::from(self.image_size) {
            return Err(E::Malformed);
        }
        let address = if let Some(module) = self.datafile {
            module.address(rva, length).map_err(|_| E::Malformed)?
        } else {
            GuestAddress(self.base.checked_add(rva).ok_or(E::Malformed)?)
        };
        if length != 0 {
            self.memory
                .check_span(address, length as usize, Access::Read)?;
        }
        Ok(address)
    }
    pub(super) fn directory_span(&self, offset: u32, length: u32) -> Result<u32, E> {
        if u64::from(offset) + u64::from(length) > u64::from(self.directory.size) {
            return Err(E::Malformed);
        }
        let rva = self.directory.rva.checked_add(offset).ok_or(E::Malformed)?;
        self.image_span(rva, length)?;
        Ok(rva)
    }
    pub(super) fn read<const N: usize>(&self, rva: u32) -> Result<[u8; N], E> {
        let address = self.image_span(rva, N as u32)?;
        let mut bytes = [0; N];
        self.memory.read(address, &mut bytes)?;
        Ok(bytes)
    }

    pub(super) fn half(&self, rva: u32) -> Result<u16, E> {
        Ok(u16::from_le_bytes(self.read(rva)?))
    }
    pub(super) fn string_length(&mut self, offset: u32) -> Result<(u32, usize), E> {
        if !offset.is_multiple_of(2) {
            return Err(E::Malformed);
        }
        let rva = self.directory_span(offset, 2)?;
        let units = self.half(rva)? as usize;
        let bytes = units.checked_mul(2).ok_or(E::Malformed)?;
        self.strings = self.strings.checked_sub(bytes).ok_or(E::Capacity)?;
        let start = offset.checked_add(2).ok_or(E::Malformed)?;
        Ok((self.directory_span(start, bytes as u32)?, units))
    }
    pub(super) fn name(&mut self, offset: u32) -> Result<Vec<u16>, E> {
        let (rva, length) = self.string_length(offset)?;
        let mut units = Vec::new();
        units.try_reserve_exact(length).map_err(|_| E::Allocation)?;
        for index in 0..length {
            units.push(self.half(rva + index as u32 * 2)?);
        }
        Ok(units)
    }
}
