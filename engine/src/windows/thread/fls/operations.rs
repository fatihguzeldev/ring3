use super::*;

impl FlsContext32 {
    fn begin(&mut self, owner: u32, kind: OperationKind32) -> Result<u64, ThreadError32> {
        Self::check_thread(owner)?;
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
        Self::check_thread(thread)?;
        if let Some(operation) = self.operations.iter().find(
            |operation| matches!(operation.kind, OperationKind32::Exit(owner) if owner == thread),
        ) {
            return Ok(Some(operation.id));
        }
        if self.row_index(thread).is_none() {
            return Ok(None);
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
            OperationKind32::Free(index) => {
                let thread = self
                    .rows
                    .iter()
                    .flatten()
                    .map(|row| row.thread)
                    .filter(|thread| *thread > operation.cursor as u32)
                    .min();
                (thread.unwrap_or(0), index, thread.is_none())
            }
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
                        row.values[slot as usize] = 0;
                    }
                    self.prune_empty_rows();
                }
                OperationKind32::Exit(thread) => {
                    if let Some(row) = self.row_index(thread) {
                        self.rows[row] = None;
                    }
                }
            }
            self.operations.swap_remove(index);
        } else if step.procedure != 0 {
            self.operations[index].active = Some(step);
        } else {
            self.clear_value(step.thread, step.index);
            let operation = &mut self.operations[index];
            operation.cursor = match operation.kind {
                OperationKind32::Free(_) => step.thread as usize,
                OperationKind32::Exit(_) => operation.cursor + 1,
            };
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
        self.clear_value(step.thread, step.index);
        let operation = self
            .operations
            .iter_mut()
            .find(|operation| operation.id == id)
            .unwrap();
        operation.active = None;
        operation.cursor = match operation.kind {
            OperationKind32::Free(_) => step.thread as usize,
            OperationKind32::Exit(_) => operation.cursor + 1,
        };
    }
}
