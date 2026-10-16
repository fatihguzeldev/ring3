use super::*;

impl FlsContext32 {
    fn begin(&mut self, owner: u32, kind: OperationKind32) -> Result<u64, ThreadError32> {
        self.row_index(owner)?;
        if self.operations.len() == 32 {
            return Err(ThreadError32::Capacity);
        }
        let id = self.next.checked_add(1).ok_or(ThreadError32::Exhausted)?;
        self.operations.push(Operation32 {
            id,
            owner,
            kind,
            cursor: 0,
            active: None,
        });
        self.next = id;
        Ok(id)
    }
    pub(crate) fn begin_free(&mut self, thread: u32, index: u32) -> Result<u64, ThreadError32> {
        let index = self.index(index)?;
        let slot = self.slots[index].ok_or(ThreadError32::InvalidParameter)?;
        if slot.freeing {
            return Err(ThreadError32::Busy);
        }
        let id = self.begin(thread, OperationKind32::Free(index as u32))?;
        self.slots[index].as_mut().unwrap().freeing = true;
        Ok(id)
    }
    pub(crate) fn begin_exit(&mut self, thread: u32) -> Result<Option<u64>, ThreadError32> {
        let row = self.row_index(thread)?;
        if self.rows[row].is_none() {
            return Ok(None);
        }
        if let Some(operation) = self.operations.iter().find(
            |operation| matches!(operation.kind, OperationKind32::Exit(owner) if owner == thread),
        ) {
            return Ok(Some(operation.id));
        }
        self.begin(thread, OperationKind32::Exit(thread)).map(Some)
    }
    pub(crate) fn step(&self, id: u64) -> Result<FlsStep32, ThreadError32> {
        let operation = self
            .operations
            .iter()
            .find(|operation| operation.id == id)
            .ok_or(ThreadError32::Stale)?;
        if operation.active.is_some() {
            return Err(ThreadError32::Busy);
        }
        let (thread, index, complete) = match operation.kind {
            OperationKind32::Free(index) => (
                operation.cursor as u32 + 1,
                index,
                operation.cursor == self.rows.len(),
            ),
            OperationKind32::Exit(thread) => (
                thread,
                operation.cursor as u32 + 1,
                operation.cursor + 1 == self.slots.len(),
            ),
        };
        let value = if complete {
            0
        } else {
            self.get(thread, index)?
        };
        let procedure = if complete || value == 0 {
            0
        } else {
            self.slots[index as usize].map_or(0, |slot| slot.callback)
        };
        Ok(FlsStep32 {
            operation: id,
            cursor: operation.cursor,
            thread,
            index,
            value,
            procedure,
            complete,
        })
    }
    pub(crate) fn check_step(&self, step: FlsStep32) -> Result<(), ThreadError32> {
        if self.step(step.operation)? != step {
            return Err(ThreadError32::Stale);
        }
        Ok(())
    }
    pub(crate) fn commit_step(&mut self, step: FlsStep32) {
        self.check_step(step)
            .expect("FLS step was preflighted under exclusive state");
        let index = self
            .operations
            .iter()
            .position(|operation| operation.id == step.operation)
            .unwrap();
        if step.complete {
            match self.operations[index].kind {
                OperationKind32::Free(slot) => {
                    self.slots[slot as usize] = None;
                    for row in self.rows.iter_mut().flatten() {
                        row[slot as usize] = 0;
                    }
                }
                OperationKind32::Exit(thread) => {
                    self.rows[thread as usize - 1] = None;
                }
            }
            self.operations.swap_remove(index);
        } else if step.procedure != 0 {
            self.operations[index].active = Some(step);
        } else {
            if let Some(row) = self.rows[step.thread as usize - 1].as_mut() {
                row[step.index as usize] = 0;
            }
            self.operations[index].cursor += 1;
        }
    }
    pub(crate) fn check_callback(&self, id: u64) -> Result<FlsStep32, ThreadError32> {
        self.operations
            .iter()
            .find(|operation| operation.id == id)
            .and_then(|operation| operation.active)
            .ok_or(ThreadError32::Stale)
    }
    pub(crate) fn complete_callback(&mut self, id: u64) {
        let step = self
            .check_callback(id)
            .expect("exact FLS callback completion was preflighted");
        if let Some(row) = self.rows[step.thread as usize - 1].as_mut() {
            row[step.index as usize] = 0;
        }
        let operation = self
            .operations
            .iter_mut()
            .find(|operation| operation.id == id)
            .unwrap();
        operation.active = None;
        operation.cursor += 1;
    }
}
