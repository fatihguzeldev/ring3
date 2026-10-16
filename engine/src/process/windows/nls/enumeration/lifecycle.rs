use crate::{
    memory::{AddressSpace, GuestAddress, PageRange},
    process::windows::nls::{NlsError32, NlsRuntime32},
};
impl NlsRuntime32 {
    pub(in crate::process) fn scratch_reservation(&self) -> PageRange {
        PageRange::new(GuestAddress(0x69f00000), 64).expect("fixed bounded nls callback arena")
    }
    pub(in crate::process) fn thread_exit_ranges(
        &self,
        memory: &AddressSpace,
        thread: u32,
    ) -> Result<Vec<PageRange>, NlsError32> {
        let mut ranges = Vec::new();
        for pending in self
            .enumerations
            .iter()
            .filter(|pending| pending.thread == thread)
        {
            pending.scratch.check(memory)?;
            ranges.try_reserve(1).map_err(|_| NlsError32::Allocation)?;
            ranges.push(pending.scratch.range());
        }
        Ok(ranges)
    }
    pub(in crate::process) fn commit_process_exit(&mut self) {
        self.enumerations.clear();
        self.locales.clear();
    }
}
