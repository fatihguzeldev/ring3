mod calls;
mod dialog;
pub(super) mod enumeration;
pub(super) mod files;
mod layout;
pub(in crate::process) mod memory;
mod paragraph;
mod queries;
pub(super) mod rendering;
mod resources;

use super::context::GdiRuntime32;
use crate::{
    memory::AddressSpace,
    windows::{
        gdi::{GdiApi32, GdiError32, GdiObject32, LogicalFont32},
        text::CodePage32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process) fn create_logical_font(
        &mut self,
        font: LogicalFont32,
    ) -> Result<u32, GdiError32> {
        font.check()?;
        let prepared = self.model.prepare_object(GdiObject32::Font(font))?;
        Ok(self.model.commit_new(prepared))
    }
    pub(super) fn logical_font(&self, handle: u32) -> Result<LogicalFont32, GdiError32> {
        match self.model.object(handle)? {
            GdiObject32::Font(font) => Ok(font),
            GdiObject32::StockFont(index) => Ok(LogicalFont32::stock(index)),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub(super) fn resolved_dc_font(
        &self,
        dc: u32,
    ) -> Result<crate::windows::gdi::ResolvedFont32, GdiError32> {
        self.resolved_state_font(self.model.state(dc)?)
    }
    pub(in crate::process::windows::gdi) fn resolved_state_font(
        &self,
        state: &crate::windows::gdi::DcState32,
    ) -> Result<crate::windows::gdi::ResolvedFont32, GdiError32> {
        if let Some(font) = &state.font_realization {
            return Ok(font.clone());
        }
        self.fonts.resolve(self.logical_font(state.font)?)
    }
    pub(super) fn resolved_dc_metrics(
        &self,
        dc: u32,
    ) -> Result<crate::windows::gdi::ResolvedFont32, GdiError32> {
        self.resolved_state_metrics(self.model.state(dc)?)
    }
    pub(in crate::process::windows::gdi) fn resolved_state_metrics(
        &self,
        state: &crate::windows::gdi::DcState32,
    ) -> Result<crate::windows::gdi::ResolvedFont32, GdiError32> {
        self.model
            .font_metric_snapshot(state.font)?
            .map_or_else(|| self.resolved_state_font(state), Ok)
    }
    pub(super) fn restore_gdi_dc(&mut self, dc: u32, saved: i32) -> Result<(), GdiError32> {
        let mut state = self.model.restored_dc_state(dc, saved)?;
        let physical = self.fonts.resolve(self.logical_font(state.font)?)?;
        self.model
            .bind_font_metric_snapshot(state.font, physical.clone())?;
        state.font_realization = Some(physical);
        self.model
            .restore_dc(dc, saved)
            .expect("exact saved dc state was preflighted");
        self.model
            .set_state(dc, state)
            .expect("restored dc remains current");
        Ok(())
    }
    pub(super) fn select_gdi_object(&mut self, dc: u32, handle: u32) -> Result<u32, GdiError32> {
        let previous = self.model.check_select(dc, handle)?;
        if previous == handle {
            return Ok(previous);
        }
        if matches!(
            self.model.object(handle)?,
            GdiObject32::Font(_) | GdiObject32::StockFont(_)
        ) {
            let resolved = self.fonts.resolve(self.logical_font(handle)?)?;
            self.model
                .bind_font_metric_snapshot(handle, resolved.clone())?;
            let mut state = self.model.state(dc)?.clone();
            state.font = handle;
            state.font_realization = Some(resolved);
            self.model.set_state(dc, state)?;
            Ok(previous)
        } else {
            self.model.select_object(dc, handle)
        }
    }
    pub(super) fn create_font(
        &mut self,
        memory: &AddressSpace,
        api: GdiApi32,
        args: &[u32],
        cp: CodePage32,
    ) -> Result<u32, GdiError32> {
        let font = if matches!(
            api,
            GdiApi32::CreateFontIndirectA | GdiApi32::CreateFontIndirectW
        ) {
            memory::logical_font(memory, args[0], api == GdiApi32::CreateFontIndirectW, cp)?
        } else {
            let face = if args[13] == 0 {
                Vec::new()
            } else {
                memory::string(memory, args[13], api == GdiApi32::CreateFontW, -1, cp, 31)?
            };
            LogicalFont32 {
                height: args[0] as i32,
                width: args[1] as i32,
                escapement: args[2] as i32,
                orientation: args[3] as i32,
                weight: args[4] as i32,
                italic: args[5] != 0,
                underline: args[6] != 0,
                strike_out: args[7] != 0,
                charset: args[8] as u8,
                out_precision: args[9] as u8,
                clip_precision: args[10] as u8,
                quality: args[11] as u8,
                pitch_family: args[12] as u8,
                face,
            }
        };
        self.create_logical_font(font)
    }
    pub(super) fn font_object(
        &self,
        memory: &mut AddressSpace,
        args: &[u32],
        wide: bool,
        cp: CodePage32,
    ) -> Result<Option<u32>, GdiError32> {
        if !matches!(
            self.model.object(args[0])?,
            GdiObject32::Font(_) | GdiObject32::StockFont(_)
        ) {
            return Ok(None);
        }
        let font = self.logical_font(args[0])?;
        let bytes = memory::encode_font(&font, wide, cp)?;
        if args[2] == 0 {
            return Ok(Some(bytes.len() as u32));
        }
        if args[1] < bytes.len() as u32 {
            return Ok(Some(0));
        }
        memory.write(crate::memory::GuestAddress(args[2]), &bytes)?;
        Ok(Some(bytes.len() as u32))
    }
}

mod rotation;
