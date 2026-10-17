use crate::{
    memory::{AddressSpace, PageRange},
    process::windows::gdi::GdiRuntime32,
    windows::gdi::GdiError32,
};
impl GdiRuntime32 {
    pub(in crate::process) fn thread_scratch_ranges(
        &self,
        memory: &AddressSpace,
        thread: u32,
    ) -> Result<Vec<PageRange>, GdiError32> {
        let mut ranges = Vec::new();
        for call in self
            .font_enumerations
            .iter()
            .filter(|call| call.thread == thread)
        {
            call.scratch.check(memory)?;
            ranges.try_reserve(1).map_err(|_| GdiError32::Allocation)?;
            ranges.push(call.scratch.range());
        }
        Ok(ranges)
    }
}
