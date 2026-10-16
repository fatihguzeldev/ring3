use super::{RegistryError32 as E, memory::output};
use crate::memory::{AddressSpace, GuestAddress};
enum Bytes32 {
    Word([u8; 4]),
    Time([u8; 8]),
    Data(Vec<u8>),
}
impl Bytes32 {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Word(bytes) => bytes,
            Self::Time(bytes) => bytes,
            Self::Data(bytes) => bytes,
        }
    }
}
pub(super) struct Outputs32 {
    entries: [Option<(GuestAddress, Bytes32)>; 12],
    count: usize,
}
impl Outputs32 {
    pub(super) fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            count: 0,
        }
    }
    fn add(&mut self, memory: &AddressSpace, address: u32, bytes: Bytes32) -> Result<(), E> {
        if address == 0 {
            return Ok(());
        }
        output(memory, address, bytes.bytes().len())?;
        if self.count == self.entries.len() {
            return Err(E::InvalidParameter);
        }
        self.entries[self.count] = Some((GuestAddress(address), bytes));
        self.count += 1;
        Ok(())
    }
    pub(super) fn word(
        &mut self,
        memory: &AddressSpace,
        address: u32,
        value: u32,
    ) -> Result<(), E> {
        self.add(memory, address, Bytes32::Word(value.to_le_bytes()))
    }
    pub(super) fn time(
        &mut self,
        memory: &AddressSpace,
        address: u32,
        value: u64,
    ) -> Result<(), E> {
        self.add(memory, address, Bytes32::Time(value.to_le_bytes()))
    }
    pub(super) fn bytes(
        &mut self,
        memory: &AddressSpace,
        address: u32,
        value: Vec<u8>,
    ) -> Result<(), E> {
        self.add(memory, address, Bytes32::Data(value))
    }
    pub(super) fn publish(self, memory: &mut AddressSpace) -> Result<(), E> {
        if self.count == 0 {
            return Ok(());
        }
        let mut spans = [(GuestAddress(0), &[][..]); 12];
        for (index, entry) in self.entries[..self.count].iter().enumerate() {
            let (address, bytes) = entry.as_ref().unwrap();
            spans[index] = (*address, bytes.bytes());
        }
        memory
            .write_spans(&spans[..self.count])
            .map_err(|_| E::NoAccess)
    }
}
