use crate::windows::commdlg::ComdlgError32 as E;

#[derive(Default)]
pub(in crate::process) struct ComdlgRuntime32 {
    errors: Vec<(u32, u32)>,
}

pub(super) struct PreparedError32 {
    thread: u32,
}

impl ComdlgRuntime32 {
    pub(super) fn error(&self, thread: u32) -> u32 {
        self.errors
            .iter()
            .find(|row| row.0 == thread)
            .map_or(0, |row| row.1)
    }

    pub(super) fn prepare_error(
        &mut self,
        thread: u32,
        thread_limit: usize,
    ) -> Result<PreparedError32, E> {
        if thread == 0 {
            return Err(E::InvalidRequest);
        }
        if !self.errors.iter().any(|row| row.0 == thread) {
            if self.errors.len() >= thread_limit {
                return Err(E::Capacity);
            }
            self.errors.try_reserve(1).map_err(|_| E::Allocation)?;
        }
        Ok(PreparedError32 { thread })
    }

    // the prepared slot is published adjacent to the accepted guest result, without yielding.
    pub(super) fn commit_error(&mut self, prepared: PreparedError32, error: u32) {
        if let Some(row) = self.errors.iter_mut().find(|row| row.0 == prepared.thread) {
            row.1 = error;
        } else {
            assert!(self.errors.len() < self.errors.capacity());
            self.errors.push((prepared.thread, error));
        }
    }

    pub(in crate::process) fn retire_thread(&mut self, thread: u32) {
        self.errors.retain(|row| row.0 != thread);
    }

    pub(in crate::process) fn terminate(&mut self) {
        self.errors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_preparation_is_neutral_and_thread_retirement_reuses_its_bound() {
        let mut runtime = ComdlgRuntime32::default();
        let prepared = runtime.prepare_error(1, 1).unwrap();
        assert_eq!(runtime.error(1), 0);
        assert!(runtime.errors.is_empty());
        runtime.commit_error(prepared, 1);
        assert_eq!(runtime.error(1), 1);
        assert_eq!(runtime.error(2), 0);
        assert!(matches!(runtime.prepare_error(2, 1), Err(E::Capacity)));
        runtime.retire_thread(1);
        let prepared = runtime.prepare_error(2, 1).unwrap();
        runtime.commit_error(prepared, 0x3003);
        assert_eq!(runtime.error(2), 0x3003);
        runtime.terminate();
        assert_eq!(runtime.error(2), 0);
    }
}
