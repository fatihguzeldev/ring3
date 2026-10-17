use crate::{
    memory::{Access, AddressSpace, GuestAddress, MemoryError, PAGE_SIZE, PageRange},
    process::{CallError, HostError},
    windows::automation::AutomationError32 as E,
};

#[derive(Clone, Copy)]
pub(super) struct PageWitness32 {
    address: GuestAddress,
    incarnation: u64,
}

impl PageWitness32 {
    fn check(self, memory: &AddressSpace) -> Result<(), HostError> {
        match memory.mapping_incarnation_at(self.address) {
            Ok(incarnation) if incarnation == self.incarnation => Ok(()),
            Ok(_) | Err(MemoryError::NotMapped { .. }) => {
                Err(HostError::Call(CallError::StateChanged))
            }
            Err(error) => Err(HostError::Memory(error)),
        }
    }
}

#[derive(Default)]
pub(super) struct ObjectWitnesses32 {
    identity: Option<u64>,
    pages: Vec<PageWitness32>,
}
impl ObjectWitnesses32 {
    pub(super) fn extend(&mut self, memory: &AddressSpace, other: Self) -> Result<(), HostError> {
        if other
            .identity
            .is_some_and(|identity| identity != memory.identity())
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        self.pages
            .try_reserve(other.pages.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for page in other.pages {
            match self
                .pages
                .binary_search_by_key(&page.address.0, |page| page.address.0)
            {
                Ok(index) => self.pages[index].check(memory)?,
                Err(index) => self.pages.insert(index, page),
            }
        }
        self.identity = Some(memory.identity());
        Ok(())
    }
    pub(super) fn reserve_query(&mut self) -> Result<(), HostError> {
        self.pages
            .try_reserve(2)
            .map_err(|_| HostError::Automation(E::Allocation))
    }
    pub(super) fn capture(
        &mut self,
        memory: &mut AddressSpace,
        object: u32,
    ) -> Result<(), HostError> {
        if self
            .identity
            .is_some_and(|identity| identity != memory.identity())
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        memory
            .check_span(GuestAddress(object), 4, Access::Read)
            .map_err(HostError::Memory)?;
        let last = object
            .checked_add(3)
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        for page in object / PAGE_SIZE..=last / PAGE_SIZE {
            let address = GuestAddress(page * PAGE_SIZE);
            match self
                .pages
                .binary_search_by_key(&address.0, |page| page.address.0)
            {
                Ok(index) => self.pages[index].check(memory)?,
                Err(index) => {
                    self.pages
                        .try_reserve(1)
                        .map_err(|_| HostError::Automation(E::Allocation))?;
                    memory
                        .observe_mapping_incarnations(
                            PageRange::new(address, 1).map_err(HostError::Memory)?,
                        )
                        .map_err(HostError::Memory)?;
                    self.pages.insert(
                        index,
                        PageWitness32 {
                            address,
                            incarnation: memory
                                .mapping_incarnation_at(address)
                                .map_err(HostError::Memory)?,
                        },
                    );
                }
            }
        }
        self.identity = Some(memory.identity());
        Ok(())
    }
    pub(super) fn check(&self, memory: &AddressSpace, object: u32) -> Result<(), HostError> {
        if self.identity != Some(memory.identity()) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let last = object
            .checked_add(3)
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        for page in object / PAGE_SIZE..=last / PAGE_SIZE {
            let address = page * PAGE_SIZE;
            let index = self
                .pages
                .binary_search_by_key(&address, |page| page.address.0)
                .map_err(|_| HostError::Call(CallError::StateChanged))?;
            self.pages[index].check(memory)?;
        }
        Ok(())
    }
}

pub(super) struct OutputWitness32 {
    identity: u64,
    address: u32,
    length: usize,
    bytes: [u8; 16],
    pages: [Option<PageWitness32>; 2],
}
impl OutputWitness32 {
    pub(super) fn prepare(
        memory: &mut AddressSpace,
        address: u32,
        length: usize,
    ) -> Result<Self, HostError> {
        if length > 16 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        memory
            .check_span(GuestAddress(address), length, Access::Write)
            .map_err(HostError::Memory)?;
        let mut bytes = [0; 16];
        memory
            .read(GuestAddress(address), &mut bytes[..length])
            .map_err(HostError::Memory)?;
        let mut pages = [None; 2];
        if length != 0 {
            let last = address
                .checked_add(length as u32 - 1)
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            for (index, page) in (address / PAGE_SIZE..=last / PAGE_SIZE).enumerate() {
                let address = GuestAddress(page * PAGE_SIZE);
                memory
                    .observe_mapping_incarnations(
                        PageRange::new(address, 1).map_err(HostError::Memory)?,
                    )
                    .map_err(HostError::Memory)?;
                pages[index] = Some(PageWitness32 {
                    address,
                    incarnation: memory
                        .mapping_incarnation_at(address)
                        .map_err(HostError::Memory)?,
                });
            }
        }
        Ok(Self {
            identity: memory.identity(),
            address,
            length,
            bytes,
            pages,
        })
    }
    pub(super) fn check(&self, memory: &AddressSpace) -> Result<(), HostError> {
        if memory.identity() != self.identity {
            return Err(HostError::Call(CallError::StateChanged));
        }
        for page in self.pages.iter().flatten() {
            page.check(memory)?;
        }
        memory
            .check_span(GuestAddress(self.address), self.length, Access::Write)
            .map_err(HostError::Memory)?;
        let mut bytes = [0; 16];
        memory
            .read(GuestAddress(self.address), &mut bytes[..self.length])
            .map_err(HostError::Memory)?;
        if bytes != self.bytes {
            return Err(HostError::Call(CallError::StateChanged));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) struct BorrowedCall32 {
    pub(super) procedure: u32,
    pub(super) object: u32,
    table: u32,
    method: u32,
    identity: u64,
    pages: [Option<PageWitness32>; 5],
}

impl BorrowedCall32 {
    pub(super) fn prepare(
        memory: &mut AddressSpace,
        object: u32,
        slot: u32,
    ) -> Result<Self, HostError> {
        if object == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let table = word(memory, object)?;
        let method = slot
            .checked_mul(4)
            .and_then(|offset| table.checked_add(offset))
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        if table == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let procedure = word(memory, method)?;
        if procedure == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        memory
            .check_span(GuestAddress(procedure), 1, Access::Execute)
            .map_err(HostError::Memory)?;
        let mut pages = [None; 5];
        let mut count = 0;
        for (address, length) in [(object, 4u32), (method, 4), (procedure, 1)] {
            let last = address
                .checked_add(length - 1)
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            for page in address / PAGE_SIZE..=last / PAGE_SIZE {
                let address = GuestAddress(page * PAGE_SIZE);
                if pages[..count].iter().any(|entry: &Option<PageWitness32>| {
                    entry.is_some_and(|entry| entry.address == address)
                }) {
                    continue;
                }
                memory
                    .observe_mapping_incarnations(
                        PageRange::new(address, 1).map_err(HostError::Memory)?,
                    )
                    .map_err(HostError::Memory)?;
                pages[count] = Some(PageWitness32 {
                    address,
                    incarnation: memory
                        .mapping_incarnation_at(address)
                        .map_err(HostError::Memory)?,
                });
                count += 1;
            }
        }
        Ok(Self {
            procedure,
            object,
            table,
            method,
            identity: memory.identity(),
            pages,
        })
    }

    pub(super) fn check(self, memory: &AddressSpace) -> Result<(), HostError> {
        if memory.identity() != self.identity {
            return Err(HostError::Call(CallError::StateChanged));
        }
        for page in self.pages.iter().flatten() {
            page.check(memory)?;
        }
        if word(memory, self.object)? != self.table || word(memory, self.method)? != self.procedure
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        memory
            .check_span(GuestAddress(self.procedure), 1, Access::Execute)
            .map_err(HostError::Memory)?;
        Ok(())
    }
}

fn word(memory: &AddressSpace, address: u32) -> Result<u32, HostError> {
    let mut bytes = [0; 4];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Permissions;

    #[test]
    fn borrowed_vtable_and_object_replacement_is_rejected_before_callback_entry() {
        let mut memory = AddressSpace::new(4).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                Permissions::READ_EXECUTE,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1100), &0x1200u32.to_le_bytes())
            .unwrap();
        memory
            .write(GuestAddress(0x1204), &0x3000u32.to_le_bytes())
            .unwrap();
        let call = BorrowedCall32::prepare(&mut memory, 0x1100, 1).unwrap();
        call.check(&memory).unwrap();
        memory.unmap_ranges_with_word_writes(&[range], &[]).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        memory
            .write(GuestAddress(0x1100), &0x1200u32.to_le_bytes())
            .unwrap();
        memory
            .write(GuestAddress(0x1204), &0x3000u32.to_le_bytes())
            .unwrap();
        let version = memory.transaction_version();
        assert!(matches!(
            call.check(&memory),
            Err(HostError::Call(CallError::StateChanged))
        ));
        assert_eq!(memory.transaction_version(), version);
    }

    #[test]
    fn callback_output_replacement_or_nested_value_write_is_rejected_without_publication() {
        let mut memory = AddressSpace::new(4).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        memory
            .write(
                GuestAddress(0x1100),
                &[3, 0, 0, 0, 0, 0, 0, 0, 37, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap();
        let witness = OutputWitness32::prepare(&mut memory, 0x1100, 16).unwrap();
        memory
            .write(GuestAddress(0x1108), &41u32.to_le_bytes())
            .unwrap();
        let version = memory.transaction_version();
        assert!(matches!(
            witness.check(&memory),
            Err(HostError::Call(CallError::StateChanged))
        ));
        assert_eq!(memory.transaction_version(), version);
        memory
            .write(GuestAddress(0x1108), &37u32.to_le_bytes())
            .unwrap();
        witness.check(&memory).unwrap();
        memory.unmap_ranges_with_word_writes(&[range], &[]).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        memory
            .write(
                GuestAddress(0x1100),
                &[3, 0, 0, 0, 0, 0, 0, 0, 37, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap();
        let version = memory.transaction_version();
        assert!(matches!(
            witness.check(&memory),
            Err(HostError::Call(CallError::StateChanged))
        ));
        assert_eq!(memory.transaction_version(), version);
    }

    #[test]
    fn repaired_output_and_method_rights_preserve_the_original_mapping_incarnation() {
        let mut memory = AddressSpace::new(4).unwrap();
        let data = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        let code = PageRange::new(GuestAddress(0x3000), 1).unwrap();
        memory.map_zeroed(data, Permissions::READ_WRITE).unwrap();
        memory.map_zeroed(code, Permissions::READ_EXECUTE).unwrap();
        memory
            .write(GuestAddress(0x1100), &0x1200u32.to_le_bytes())
            .unwrap();
        memory
            .write(GuestAddress(0x1204), &0x3000u32.to_le_bytes())
            .unwrap();
        let output = OutputWitness32::prepare(&mut memory, 0x1300, 16).unwrap();
        let call = BorrowedCall32::prepare(&mut memory, 0x1100, 1).unwrap();
        memory.protect(data, Permissions::READ).unwrap();
        assert!(matches!(output.check(&memory), Err(HostError::Memory(_))));
        call.check(&memory).unwrap();
        memory.protect(data, Permissions::READ_WRITE).unwrap();
        output.check(&memory).unwrap();
        memory.protect(code, Permissions::READ).unwrap();
        assert!(matches!(call.check(&memory), Err(HostError::Memory(_))));
        memory.protect(code, Permissions::READ_EXECUTE).unwrap();
        call.check(&memory).unwrap();
        output.check(&memory).unwrap();
    }
}
