use super::*;

impl EngineInstance {
    /// stack reads belong to the generated cpu_read phase; this helper publishes only code state.
    pub fn far_return(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        self.guard(self.key, self.generation())?;
        self.far_return_bound(offset, selector, word)
    }

    pub fn far_return_resident(
        &mut self,
        key: u64,
        id: u64,
        offset: u32,
        selector: u32,
        word: u32,
    ) -> Result<(), HostError> {
        self.guard_resident(key, id)?;
        self.far_return_bound(offset, selector, word)
    }

    fn far_return_bound(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        let width = width(word)?;
        let selector = u16::try_from(selector).map_err(|_| HostError::InvalidRequest)?;
        let context = self.segment_context()?;
        match self.segment_descriptors.resolve_far_return(
            selector,
            offset,
            width,
            USER_MODE_ENVIRONMENT.current_privilege(),
        ) {
            Ok(transfer) => {
                let prepared = self.prepare_code_transfer(context, transfer)?;
                self.commit_code_transfer(prepared);
                Ok(())
            }
            Err(error) => self.code_transfer_fault(error),
        }
    }
}

#[cfg(test)]
#[path = "return/tests.rs"]
mod tests;
