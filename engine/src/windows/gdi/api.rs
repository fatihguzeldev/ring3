use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};

macro_rules! gdi_apis {
    ($($name:ident,$words:literal;)+) => {
        #[derive(Clone,Copy,Debug,PartialEq,Eq)]
        #[repr(u32)]
        pub enum GdiApi32 { $($name,)+ }
        impl GdiApi32 {
            pub const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub fn from_id(id: u32) -> Option<Self> { Self::ALL.get(id.checked_sub(0x001a_0001)? as usize).copied() }
            pub fn id(self) -> u32 { 0x001a_0001 + self as u32 }
            pub fn name(self) -> &'static str { match self { $(Self::$name => stringify!($name),)+ } }
            pub fn stack_words(self) -> u32 { match self { $(Self::$name => $words,)+ } }
        }
    }
}
gdi_apis! {
    CreateCompatibleDC,1; CreateDCA,4; CreateDCW,4; DeleteDC,1;
    SaveDC,1; RestoreDC,2; SelectObject,2; GetCurrentObject,2; GetObjectType,1;
    GetStockObject,1; DeleteObject,1; GetObjectA,3; GetObjectW,3;
    CreatePen,3; CreatePenIndirect,1; CreateSolidBrush,1; CreateHatchBrush,2; CreateBrushIndirect,1;
    CreateBitmap,5; CreateBitmapIndirect,1; CreateCompatibleBitmap,3; CreateDIBSection,6;
    GetBitmapBits,3; SetBitmapBits,3; GetDIBits,7; SetDIBits,7;
    GetDIBColorTable,4; SetDIBColorTable,4; GdiFlush,0;
    BitBlt,9; StretchBlt,11; PatBlt,6; StretchDIBits,13; SetDIBitsToDevice,12;
    GetPixel,3; SetPixel,4; SetPixelV,4; MoveToEx,4; LineTo,3; Rectangle,5;
    SetTextColor,2; GetTextColor,1; SetBkColor,2; GetBkColor,1; SetBkMode,2; GetBkMode,1;
    SetDCBrushColor,2; GetDCBrushColor,1; SetDCPenColor,2; GetDCPenColor,1;
    SetROP2,2; GetROP2,1; SetStretchBltMode,2; GetStretchBltMode,1;
    SetMapMode,2; GetMapMode,1; SetWindowOrgEx,4; GetWindowOrgEx,2;
    SetViewportOrgEx,4; GetViewportOrgEx,2; SetWindowExtEx,4; GetWindowExtEx,2;
    SetViewportExtEx,4; GetViewportExtEx,2; OffsetWindowOrgEx,4; OffsetViewportOrgEx,4;
    SetBrushOrgEx,4; GetBrushOrgEx,2; LPtoDP,3; DPtoLP,3; GetDeviceCaps,2;
    CreateRectRgn,4; CreateRectRgnIndirect,1; SetRectRgn,5; CombineRgn,4;
    OffsetRgn,3; EqualRgn,2; GetRgnBox,2; PtInRegion,3; RectInRegion,2; GetRegionData,3;
    SelectClipRgn,2; ExtSelectClipRgn,3; IntersectClipRect,5; ExcludeClipRect,5;
    GetClipBox,2; GetClipRgn,2; PtVisible,3; RectVisible,2;
    GetDC,1; GetWindowDC,1; ReleaseDC,2; WindowFromDC,1;
    BeginPaint,2; EndPaint,2; InvalidateRect,3; ValidateRect,2; GetUpdateRect,3; UpdateWindow,1;
    FillRect,3; FrameRect,3; InvertRect,2;
    CreateFontA,14; CreateFontW,14; CreateFontIndirectA,1; CreateFontIndirectW,1;
    GetTextMetricsA,2; GetTextMetricsW,2; GetTextFaceA,3; GetTextFaceW,3;
    GetTextExtentPoint32A,4; GetTextExtentPoint32W,4; GetTextExtentExPointA,7; GetTextExtentExPointW,7;
    GetCharWidth32A,4; GetCharWidth32W,4; GetCharABCWidthsA,4; GetCharABCWidthsW,4;
    GetGlyphIndicesA,5; GetGlyphIndicesW,5;
    TextOutA,5; TextOutW,5; ExtTextOutA,8; ExtTextOutW,8;
    GetTextAlign,1; SetTextAlign,2; GetTextCharacterExtra,1; SetTextCharacterExtra,2; SetTextJustification,3;
    DrawTextA,5; DrawTextW,5; DrawTextExA,6; DrawTextExW,6;
    GetSysColor,1; GetSysColorBrush,1;
    CreatePalette,1; SelectPalette,3; GetPaletteEntries,4; SetPaletteEntries,4;
    ResizePalette,2; GetNearestPaletteIndex,2; RealizePalette,1; GetSystemPaletteEntries,4;
    AddFontMemResourceEx,4; RemoveFontMemResourceEx,1;
    EnumFontsA,4; EnumFontsW,4; EnumFontFamiliesA,4; EnumFontFamiliesW,4;
    EnumFontFamiliesExA,5; EnumFontFamiliesExW,5;
    AddFontResourceA,1; AddFontResourceW,1; RemoveFontResourceA,1; RemoveFontResourceW,1;
    AddFontResourceExA,3; AddFontResourceExW,3; RemoveFontResourceExA,3; RemoveFontResourceExW,3;
    AlphaBlend,11; TransparentBlt,11; GradientFill,6;
    CreatePatternBrush,1; GetDCEx,3; ExtCreatePen,5; DrawEdge,4; DrawFrameControl,4; CreateDIBPatternBrushPt,2; CreateDIBPatternBrush,2;
}
impl GdiApi32 {
    pub fn msimg(self) -> bool {
        matches!(
            self,
            Self::AlphaBlend | Self::TransparentBlt | Self::GradientFill
        )
    }
    pub fn font_enumeration(self) -> bool {
        matches!(
            self,
            Self::EnumFontsA
                | Self::EnumFontsW
                | Self::EnumFontFamiliesA
                | Self::EnumFontFamiliesW
                | Self::EnumFontFamiliesExA
                | Self::EnumFontFamiliesExW
        )
    }
    pub fn font_file_resource(self) -> bool {
        matches!(
            self,
            Self::AddFontResourceA
                | Self::AddFontResourceW
                | Self::RemoveFontResourceA
                | Self::RemoveFontResourceW
                | Self::AddFontResourceExA
                | Self::AddFontResourceExW
                | Self::RemoveFontResourceExA
                | Self::RemoveFontResourceExW
        )
    }
    pub fn font_memory_resource(self) -> bool {
        matches!(
            self,
            Self::AddFontMemResourceEx | Self::RemoveFontMemResourceEx
        )
    }
    pub fn user32(self) -> bool {
        matches!(
            self,
            Self::DrawEdge
                | Self::DrawFrameControl
                | Self::GetDCEx
                | Self::GetSysColor
                | Self::GetSysColorBrush
                | Self::DrawTextA
                | Self::DrawTextW
                | Self::DrawTextExA
                | Self::DrawTextExW
                | Self::GetDC
                | Self::GetWindowDC
                | Self::ReleaseDC
                | Self::WindowFromDC
                | Self::BeginPaint
                | Self::EndPaint
                | Self::InvalidateRect
                | Self::ValidateRect
                | Self::GetUpdateRect
                | Self::UpdateWindow
                | Self::FillRect
                | Self::FrameRect
                | Self::InvertRect
        )
    }
    pub fn failure_value(self) -> u32 {
        if matches!(
            self,
            Self::GetTextCharacterExtra | Self::SetTextCharacterExtra
        ) {
            return 0x8000_0000;
        }
        if matches!(
            self,
            Self::GetGlyphIndicesA
                | Self::GetGlyphIndicesW
                | Self::RealizePalette
                | Self::GetNearestPaletteIndex
                | Self::SetTextAlign
                | Self::GetTextAlign
                | Self::GetPixel
                | Self::SetPixel
                | Self::SetTextColor
                | Self::GetTextColor
                | Self::SetBkColor
                | Self::GetBkColor
                | Self::SetDCBrushColor
                | Self::GetDCBrushColor
                | Self::SetDCPenColor
                | Self::GetDCPenColor
        ) {
            u32::MAX
        } else {
            0
        }
    }
}
fn export(api: GdiApi32) -> SystemExport32 {
    SystemExport32 {
        name: Some(api.name().into()),
        ordinal: None,
        target: SystemExportTarget32::Gate(api.id()),
    }
}
pub fn gdi32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "gdi32.dll".into(),
        preferred_base,
        exports: GdiApi32::ALL
            .iter()
            .copied()
            .filter(|api| !api.user32() && !api.msimg())
            .map(export)
            .collect(),
    }
}
pub fn user32_gdi_exports() -> impl Iterator<Item = SystemExport32> {
    GdiApi32::ALL
        .iter()
        .copied()
        .filter(|api| api.user32())
        .map(export)
}

pub fn msimg32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "msimg32.dll".into(),
        preferred_base,
        exports: GdiApi32::ALL
            .iter()
            .copied()
            .filter(|api| api.msimg())
            .map(export)
            .collect(),
    }
}
