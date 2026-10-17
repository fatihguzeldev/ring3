use super::super::context::GdiRuntime32;
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::gdi::{GdiApi32, GdiError32},
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn memory_font_resource(
        &mut self,
        ram: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if api == GdiApi32::RemoveFontMemResourceEx {
            self.fonts.remove_memory_font(args[0])?;
            return Ok(1);
        }
        if api != GdiApi32::AddFontMemResourceEx
            || args[2] != 0
            || args[1] == 0
            || args[1] > 16 << 20
            || args[3] == 0
        {
            return Err(GdiError32::InvalidRequest);
        }
        ram.check_span(GuestAddress(args[3]), 4, Access::Write)?;
        ram.ensure_transaction_versions(1)?;
        let mut bytes = super::super::memory::zeroes(args[1] as usize)?;
        ram.read(GuestAddress(args[0]), &mut bytes)?;
        let prepared = self.fonts.prepare_font_source(&bytes, false)?;
        let id = prepared.handle();
        let count = prepared.count();
        self.fonts.check_font_source(&prepared)?;
        ram.write(GuestAddress(args[3]), &count.to_le_bytes())?;
        self.fonts.commit_font_source(prepared);
        Ok(id)
    }
}
