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
    pub(super) fn check_thread(thread: u32) -> Result<(), ThreadError32> {
        if thread == 0 {
            Err(ThreadError32::InvalidParameter)
        } else {
            Ok(())
        }
    }
    pub(super) fn row_index(&self, thread: u32) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.as_ref().is_some_and(|row| row.thread == thread))
    }
    pub(super) fn index(&self, index: u32) -> Result<usize, ThreadError32> {
        if index == 0 || index as usize >= self.slots.len() {
            return Err(ThreadError32::InvalidParameter);
        }
        Ok(index as usize)
    }
    fn ensure_row(&mut self, thread: u32) -> Result<usize, ThreadError32> {
        if let Some(index) = self.row_index(thread) {
            return Ok(index);
        }
        let index = self
            .rows
            .iter()
            .position(Option::is_none)
            .ok_or(ThreadError32::Capacity)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.slots.len())
            .map_err(|_| ThreadError32::Allocation)?;
        values.resize(self.slots.len(), 0);
        self.rows[index] = Some(Row32 { thread, values });
        Ok(index)
    }
    pub(super) fn prune_empty_rows(&mut self) {
        for row in &mut self.rows {
            if row
                .as_ref()
                .is_some_and(|row| row.values.iter().all(|value| *value == 0))
            {
                *row = None;
            }
        }
    }
    pub(super) fn clear_value(&mut self, thread: u32, index: u32) {
        if let Some(slot) = self.row_index(thread) {
            let row = self.rows[slot].as_mut().unwrap();
            row.values[index as usize] = 0;
            if row.values.iter().all(|value| *value == 0) {
                self.rows[slot] = None;
            }
        }
    }
    pub(crate) fn allocate(&mut self, thread: u32, callback: u32) -> Result<u32, ThreadError32> {
        Self::check_thread(thread)?;
        let index = self
            .slots
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, slot)| slot.is_none())
            .map(|(index, _)| index)
            .ok_or(ThreadError32::Capacity)?;
        for row in self.rows.iter_mut().flatten() {
            row.values[index] = 0;
        }
        self.prune_empty_rows();
        self.slots[index] = Some(Slot32 {
            callback,
            freeing: false,
        });
        Ok(index as u32)
    }
    pub(crate) fn get(&self, thread: u32, index: u32) -> Result<u32, ThreadError32> {
        let index = self.index(index)?;
        Self::check_thread(thread)?;
        Ok(self
            .row_index(thread)
            .map_or(0, |row| self.rows[row].as_ref().unwrap().values[index]))
    }
    pub(crate) fn set(&mut self, thread: u32, index: u32, value: u32) -> Result<(), ThreadError32> {
        let index = self.index(index)?;
        Self::check_thread(thread)?;
        if value == 0 {
            self.clear_value(thread, index as u32);
            return Ok(());
        }
        let row = self.ensure_row(thread)?;
        self.rows[row].as_mut().unwrap().values[index] = value;
        Ok(())
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
            && self.operations.is_empty()
            && self.rows.iter().all(Option::is_none)
    }
    pub(crate) fn retire_thread(&mut self, thread: u32) {
        if let Some(index) = self.row_index(thread) {
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
