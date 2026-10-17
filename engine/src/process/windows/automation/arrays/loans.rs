use crate::{
    memory::{Access, GuestAddress, PAGE_SIZE, PageRange, WordWrite32},
    process::{CallError, EngineInstance, HostError},
    windows::automation::AutomationError32 as E,
};

pub(in crate::process::windows::automation) struct ArrayLoan32 {
    descriptor: u32,
    base: u32,
    allocation: u64,
    data: Option<(u32, u64)>,
    identity: u64,
    pages: Vec<(GuestAddress, u64)>,
    header: Vec<u8>,
    borrowed: bool,
}

pub(in crate::process) struct PreparedAutomationArrays32 {
    words: Vec<WordWrite32>,
    rows: Vec<(u32, u32, bool)>,
}
impl PreparedAutomationArrays32 {
    pub(in crate::process) fn words(&self) -> &[WordWrite32] {
        &self.words
    }
}

impl EngineInstance {
    pub(in crate::process::windows::automation) fn prepare_automation_array_loan(
        &mut self,
        descriptor: u32,
        borrowed: bool,
    ) -> Result<ArrayLoan32, HostError> {
        let (index, locks) = self.automation_array_index(descriptor)?;
        let array = &self.automation.arrays[index];
        if array.exclusive || (!borrowed && locks != 0) {
            return Err(HostError::Automation(E::ArrayLocked));
        }
        if borrowed {
            locks
                .checked_add(1)
                .ok_or(HostError::Automation(E::Unexpected))?;
            array
                .borrows
                .checked_add(1)
                .ok_or(HostError::Automation(E::Capacity))?;
        }
        let base = array.base;
        let allocation = self.windows_runtime_allocation_identity(base)?;
        let data = array
            .separate_data
            .then(|| {
                self.windows_runtime_allocation_identity(array.data)
                    .map(|identity| (array.data, identity))
            })
            .transpose()?;
        let spans = [
            (descriptor, array.layout.descriptor_bytes()),
            (array.data, array.layout.data_bytes),
        ];
        let mut header = Vec::new();
        header
            .try_reserve_exact(spans[0].1)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        header.resize(spans[0].1, 0);
        self.memory()?
            .read(GuestAddress(descriptor), &mut header)
            .map_err(HostError::Memory)?;
        header[8..12].fill(0);
        let mut pages: Vec<(GuestAddress, u64)> = Vec::new();
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        for (address, length) in spans {
            if length == 0 {
                continue;
            }
            memory
                .check_span(GuestAddress(address), length, Access::Read)
                .map_err(HostError::Memory)?;
            let last = address
                .checked_add(length as u32 - 1)
                .ok_or(HostError::Automation(E::Overflow))?;
            for page in address / PAGE_SIZE..=last / PAGE_SIZE {
                let address = GuestAddress(page * PAGE_SIZE);
                let index = match pages.binary_search_by_key(&address.0, |row| row.0.0) {
                    Ok(_) => continue,
                    Err(index) => index,
                };
                pages
                    .try_reserve(1)
                    .map_err(|_| HostError::Automation(E::Allocation))?;
                memory
                    .observe_mapping_incarnations(
                        PageRange::new(address, 1).map_err(HostError::Memory)?,
                    )
                    .map_err(HostError::Memory)?;
                pages.insert(
                    index,
                    (
                        address,
                        memory
                            .mapping_incarnation_at(address)
                            .map_err(HostError::Memory)?,
                    ),
                );
            }
        }
        Ok(ArrayLoan32 {
            descriptor,
            base,
            allocation,
            data,
            identity: memory.identity(),
            pages,
            header,
            borrowed,
        })
    }
    pub(in crate::process::windows::automation) fn commit_automation_array_loan(
        &mut self,
        loan: &ArrayLoan32,
    ) -> Result<(), HostError> {
        let (index, locks) = self.automation_array_index(loan.descriptor)?;
        if loan.borrowed {
            self.memory
                .as_mut()
                .unwrap()
                .write_words32(&[WordWrite32 {
                    address: GuestAddress(loan.descriptor + 8),
                    value: locks
                        .checked_add(1)
                        .ok_or(HostError::Automation(E::Unexpected))?,
                }])
                .map_err(HostError::Memory)?;
            self.automation.arrays[index].borrows += 1;
        } else {
            self.automation.arrays[index].exclusive = true;
        }
        Ok(())
    }
    pub(in crate::process::windows::automation) fn check_automation_array_loan(
        &self,
        loan: &ArrayLoan32,
    ) -> Result<u32, HostError> {
        let state = || HostError::Call(CallError::StateChanged);
        let memory = self.memory()?;
        if memory.identity() != loan.identity
            || self.windows_runtime_allocation_identity(loan.base).ok() != Some(loan.allocation)
            || loan.data.is_some_and(|(pointer, identity)| {
                self.windows_runtime_allocation_identity(pointer).ok() != Some(identity)
            })
        {
            return Err(state());
        }
        for &(address, incarnation) in &loan.pages {
            if memory.mapping_incarnation_at(address).ok() != Some(incarnation) {
                return Err(state());
            }
        }
        let (index, locks) = self.automation_array_index(loan.descriptor)?;
        let array = &self.automation.arrays[index];
        if loan.borrowed && (array.borrows == 0 || locks < array.borrows)
            || !loan.borrowed && !array.exclusive
        {
            return Err(state());
        }
        let mut bytes = [0; 528];
        memory
            .read(
                GuestAddress(loan.descriptor),
                &mut bytes[..loan.header.len()],
            )
            .map_err(HostError::Memory)?;
        bytes[8..12].fill(0);
        if bytes[..loan.header.len()] != loan.header {
            return Err(state());
        }
        Ok(locks)
    }
    pub(in crate::process::windows::automation) fn automation_array_unlock_word(
        &self,
        loan: &ArrayLoan32,
    ) -> Result<Option<WordWrite32>, HostError> {
        let locks = self.check_automation_array_loan(loan)?;
        Ok(loan.borrowed.then(|| WordWrite32 {
            address: GuestAddress(loan.descriptor + 8),
            value: locks - 1,
        }))
    }
    pub(in crate::process::windows::automation) fn commit_automation_array_unlock(
        &mut self,
        loan: ArrayLoan32,
    ) {
        if let Some(array) = self
            .automation
            .arrays
            .iter_mut()
            .find(|array| array.descriptor == loan.descriptor)
        {
            if loan.borrowed {
                array.borrows -= 1;
            } else {
                array.exclusive = false;
            }
        }
    }
    pub(in crate::process) fn prepare_automation_array_retirement(
        &self,
        thread: Option<u32>,
    ) -> Result<PreparedAutomationArrays32, HostError> {
        let mut rows: Vec<(u32, u32, bool)> = Vec::new();
        for work in self
            .automation
            .references
            .iter()
            .filter(|work| thread.is_none_or(|thread| work.thread == thread))
        {
            if let Some(loan) = &work.array {
                self.check_automation_array_loan(loan)?;
                if let Some(row) = rows.iter_mut().find(|row| row.0 == loan.descriptor) {
                    row.1 = row
                        .1
                        .checked_add(u32::from(loan.borrowed))
                        .ok_or(HostError::Automation(E::Capacity))?;
                    row.2 |= !loan.borrowed;
                } else {
                    rows.try_reserve(1)
                        .map_err(|_| HostError::Automation(E::Allocation))?;
                    rows.push((loan.descriptor, u32::from(loan.borrowed), !loan.borrowed));
                }
            }
        }
        let mut words = Vec::new();
        words
            .try_reserve_exact(rows.len())
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for &(descriptor, count, _) in &rows {
            let (_, locks) = self.automation_array_index(descriptor)?;
            if count != 0 {
                words.push(WordWrite32 {
                    address: GuestAddress(descriptor + 8),
                    value: locks
                        .checked_sub(count)
                        .ok_or(HostError::Call(CallError::StateChanged))?,
                });
            }
        }
        words.sort_unstable_by_key(|word| word.address.0);
        Ok(PreparedAutomationArrays32 { words, rows })
    }
    pub(in crate::process) fn commit_automation_array_retirement(
        &mut self,
        prepared: PreparedAutomationArrays32,
    ) {
        for (descriptor, count, exclusive) in prepared.rows {
            if let Some(array) = self
                .automation
                .arrays
                .iter_mut()
                .find(|array| array.descriptor == descriptor)
            {
                array.borrows -= count;
                if exclusive {
                    array.exclusive = false;
                }
            }
        }
    }
}
