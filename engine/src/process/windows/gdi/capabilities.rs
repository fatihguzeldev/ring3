use super::context::GdiRuntime32;
use crate::windows::gdi::{GdiApi32, GdiError32};

impl GdiRuntime32 {
    pub(super) fn unavailable_capability(
        &self,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        use GdiApi32::*;
        match api {
            CreateMetaFileA => Err(GdiError32::Unsupported),
            CloseMetaFile | CopyMetaFileA | DeleteMetaFile => Err(GdiError32::InvalidHandle),
            EnumMetaFile | PlayMetaFile => {
                self.model.dc_kind(args[0])?;
                Err(GdiError32::InvalidHandle)
            }
            SelectClipPath => {
                self.model.dc_kind(args[0])?;
                if !(1..=5).contains(&args[1]) {
                    return Err(GdiError32::InvalidRequest);
                }
                Err(GdiError32::NoPath)
            }
            AbortDoc | EndDoc | EndPage | Escape | SetAbortProc | StartDocA | StartPage | ArcTo
            | PlayMetaFileRecord | PolyBezierTo | PolyDraw | SetColorAdjustment => {
                self.model.dc_kind(args[0])?;
                Err(GdiError32::Unsupported)
            }
            _ => Err(GdiError32::InvalidRequest),
        }
    }
}
