use super::*;

impl FlsContext32 {
    pub(crate) fn new(indices: u32, threads: u32) -> Result<Self, ThreadError32> {
        if !(2..=4096).contains(&indices) || !(1..=4096).contains(&threads) {
            return Err(ThreadError32::InvalidLimits);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(indices as usize)
            .map_err(|_| ThreadError32::Allocation)?;
        slots.resize(indices as usize, None);
        let mut rows = Vec::new();
        rows.try_reserve_exact(threads as usize)
            .map_err(|_| ThreadError32::Allocation)?;
        rows.resize_with(threads as usize, || None);
        let mut operations = Vec::new();
        operations
            .try_reserve_exact(32)
            .map_err(|_| ThreadError32::Allocation)?;
        Ok(Self {
            slots,
            rows,
            operations,
            next: 0,
        })
    }
    pub(super) fn row_index(&self, thread: u32) -> Result<usize, ThreadError32> {
        let index = thread
            .checked_sub(1)
            .ok_or(ThreadError32::InvalidParameter)? as usize;
        if index >= self.rows.len() {
            return Err(ThreadError32::InvalidParameter);
        }
        Ok(index)
    }
    pub(super) fn index(&self, index: u32) -> Result<usize, ThreadError32> {
        if index == 0 || index as usize >= self.slots.len() {
            return Err(ThreadError32::InvalidParameter);
        }
        Ok(index as usize)
    }
    fn ensure_row(&mut self, thread: u32) -> Result<usize, ThreadError32> {
        let index = self.row_index(thread)?;
        if self.rows[index].is_none() {
            let mut row = Vec::new();
            row.try_reserve_exact(self.slots.len())
                .map_err(|_| ThreadError32::Allocation)?;
            row.resize(self.slots.len(), 0);
            self.rows[index] = Some(row);
        }
        Ok(index)
    }
    pub(crate) fn allocate(&mut self, thread: u32, callback: u32) -> Result<u32, ThreadError32> {
        let index = self
            .slots
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, slot)| slot.is_none())
            .map(|(index, _)| index)
            .ok_or(ThreadError32::Capacity)?;
        self.ensure_row(thread)?;
        for row in self.rows.iter_mut().flatten() {
            row[index] = 0;
        }
        self.slots[index] = Some(Slot32 {
            callback,
            freeing: false,
        });
        Ok(index as u32)
    }
    pub(crate) fn get(&self, thread: u32, index: u32) -> Result<u32, ThreadError32> {
        let index = self.index(index)?;
        let row = self.row_index(thread)?;
        Ok(self.rows[row].as_ref().map_or(0, |row| row[index]))
    }
    pub(crate) fn set(&mut self, thread: u32, index: u32, value: u32) -> Result<(), ThreadError32> {
        let index = self.index(index)?;
        let row = self.ensure_row(thread)?;
        self.rows[row].as_mut().unwrap()[index] = value;
        Ok(())
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
            && self.operations.is_empty()
            && self
                .rows
                .iter()
                .flatten()
                .all(|row| row.iter().all(|&value| value == 0))
    }
    pub(crate) fn retire_thread(&mut self, thread: u32) {
        if let Ok(index) = self.row_index(thread) {
            self.rows[index] = None;
        }
        for operation in self
            .operations
            .iter()
            .filter(|operation| operation.owner == thread)
        {
            if let OperationKind32::Free(index) = operation.kind
                && let Some(slot) = self.slots[index as usize].as_mut()
            {
                slot.freeing = false;
            }
        }
        self.operations
            .retain(|operation| operation.owner != thread);
    }
}
