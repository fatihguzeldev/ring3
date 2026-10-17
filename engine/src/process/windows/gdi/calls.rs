use super::{context::GdiRuntime32, memory, windows::GdiWindow32};
use crate::{
    memory::AddressSpace,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        gdi::{BitmapLayout32, DcKind32, GdiApi32, GdiError32},
        user32::User32Api32,
    },
};

impl EngineInstance {
    pub(in crate::process) fn gdi_font_provider_owns_token(&self, token: u32) -> bool {
        self.gdi.as_ref().is_some_and(|gdi| {
            gdi.font_enumerations.iter().any(|call| call.token == token)
                || gdi.font_file_calls.iter().any(|call| call.token == token)
        })
    }
    pub(in crate::process) fn try_complete_gdi_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let api = GdiApi32::from_id(id);
        let erase = matches!(
            User32Api32::from_id(id),
            Some(User32Api32::DefWindowProcA | User32Api32::DefWindowProcW)
        ) && frame.arguments().get(1) == Some(&0x14);
        if let Some(api) = api
            && api.font_file_resource()
        {
            return self.complete_gdi_file_font(owner, token, api, frame);
        }
        if let Some(api) = api
            && api.font_enumeration()
        {
            return self.complete_gdi_font_enumeration(owner, token, api, frame);
        }
        if api.is_none() && !erase {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.map_or(4, GdiApi32::stack_words)
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        let thread = self.current_windows_thread_id()?;
        let context = self.user32.as_ref();
        if let Some(runtime) = self.gdi.as_mut() {
            runtime.retire_dead_windows(|window| {
                context.is_some_and(|context| {
                    context
                        .windows()
                        .get(crate::windows::window::WindowHandle32::try_from(window).unwrap())
                        .is_ok()
                })
            });
        }
        if let Some(api) = api
            && matches!(
                api,
                GdiApi32::BeginPaint
                    | GdiApi32::EndPaint
                    | GdiApi32::InvalidateRect
                    | GdiApi32::ValidateRect
                    | GdiApi32::GetUpdateRect
                    | GdiApi32::UpdateWindow
            )
        {
            return self.complete_gdi_paint(owner, token, api, frame);
        }
        if let Some(api) = api
            && self.complete_gdi_section(owner, token, api, frame)?
        {
            return Ok(true);
        }
        let args = frame.arguments();
        let result = if erase {
            self.gdi_erase_background(args)
        } else if let Some(result) = self.gdi_global_pattern_call(api.unwrap(), args) {
            result
        } else if api == Some(GdiApi32::GetDCEx) {
            self.gdi_dc_ex(args, thread)
        } else if matches!(api, Some(GdiApi32::GetDC | GdiApi32::GetWindowDC)) {
            let whole = api == Some(GdiApi32::GetWindowDC);
            let window = if args[0] == 0 {
                let config = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?.config();
                Ok(GdiWindow32 {
                    window: 0,
                    thread,
                    class_atom: 0,
                    class_style: 0,
                    background_brush: 0,
                    width: config.screen_width,
                    height: config.screen_height,
                    whole,
                })
            } else {
                self.user32
                    .as_ref()
                    .ok_or(HostError::InvalidRequest)?
                    .gdi_window(args[0], whole)
                    .map_err(|_| GdiError32::InvalidHandle)
            };
            window.and_then(|window| {
                self.gdi
                    .as_mut()
                    .ok_or(GdiError32::InvalidConfig)?
                    .acquire_dc(window, thread)
            })
        } else {
            let ansi = self.windows_ansi_code_page();
            self.gdi.as_mut().ok_or(HostError::InvalidRequest)?.execute(
                self.memory.as_mut().ok_or(HostError::Closed)?,
                api.unwrap(),
                args,
                thread,
                ansi,
            )
        };
        let (value, error) = match result {
            Ok(value) => (value, None),
            Err(GdiError32::Memory(
                error @ (crate::memory::MemoryError::VersionExhausted
                | crate::memory::MemoryError::Allocation),
            )) => return Err(HostError::Memory(error)),
            Err(error) => (
                api.map_or(0, GdiApi32::failure_value),
                if api.is_some_and(GdiApi32::font_memory_resource) {
                    None
                } else {
                    Some(error.win32())
                },
            ),
        };
        let prepared = self.prepare_gdi_result(api, frame, value, error)?;
        self.publish_prepared_call(prepared);
        Ok(true)
    }

    pub(super) fn prepare_gdi_result(
        &mut self,
        api: Option<GdiApi32>,
        frame: CallFrame32,
        value: u32,
        error: Option<u32>,
    ) -> Result<crate::process::calls::call::PreparedCall32, HostError> {
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        let thread = error
            .map(|error| {
                self.current_windows_thread()
                    .map(|thread| thread.with_last_error(error))
            })
            .transpose()?;
        if api == Some(GdiApi32::CreateDIBSection) && value == 0 {
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            let output = crate::memory::GuestAddress(frame.arguments()[3]);
            if memory
                .check_span(output, 4, crate::memory::Access::Write)
                .is_ok()
            {
                memory
                    .ensure_transaction_versions(2)
                    .map_err(HostError::Memory)?;
                memory
                    .write(output, &0u32.to_le_bytes())
                    .map_err(HostError::Memory)?;
            }
        }
        if let Some(thread) = thread {
            self.publish_windows_thread(thread)?;
        }
        Ok(prepared)
    }
}
impl GdiRuntime32 {
    pub(super) fn execute(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
        thread: u32,
        ansi: crate::windows::text::CodePage32,
    ) -> Result<u32, GdiError32> {
        use GdiApi32::*;
        match api {
            AddFontMemResourceEx | RemoveFontMemResourceEx => {
                self.memory_font_resource(memory, api, args)
            }
            AddFontResourceA
            | AddFontResourceW
            | RemoveFontResourceA
            | RemoveFontResourceW
            | AddFontResourceExA
            | AddFontResourceExW
            | RemoveFontResourceExA
            | RemoveFontResourceExW => Err(GdiError32::InvalidRequest),
            EnumFontsA | EnumFontsW | EnumFontFamiliesA | EnumFontFamiliesW
            | EnumFontFamiliesExA | EnumFontFamiliesExW => Err(GdiError32::InvalidRequest),
            GetSysColor => Ok(self.system_color(args[0])),
            GetSysColorBrush => {
                Ok(crate::windows::gdi::GdiContext32::system_brush(args[0]).unwrap_or(0))
            }
            CreatePalette
            | SelectPalette
            | GetPaletteEntries
            | SetPaletteEntries
            | ResizePalette
            | GetNearestPaletteIndex
            | RealizePalette
            | GetSystemPaletteEntries => self.palette_call(memory, api, args),
            CreateCompatibleDC => self.memory_dc(args[0], thread),
            CreateDCA | CreateDCW => {
                let driver = if args[0] == 0 { args[1] } else { args[0] };
                let name = super::super::strings::Input32::read(
                    memory,
                    driver,
                    api == CreateDCW,
                    ansi,
                    255,
                )
                .map_err(|error| match error {
                    super::super::strings::WindowsStringError32::Memory(error) => {
                        GdiError32::Memory(error)
                    }
                    _ => GdiError32::InvalidRequest,
                })?;
                if String::from_utf16_lossy(&name.units).eq_ignore_ascii_case("DISPLAY") {
                    self.screen_dc()
                } else {
                    Err(GdiError32::Unsupported)
                }
            }
            DeleteDC => {
                self.model.delete_dc(args[0])?;
                self.prune_defaults();
                Ok(1)
            }
            SaveDC => self.model.save_dc(args[0]),
            RestoreDC => self.restore_gdi_dc(args[0], args[1] as i32).map(|()| 1),
            SelectObject => self.select_gdi_object(args[0], args[1]),
            GetCurrentObject => {
                let state = self.model.state(args[0])?;
                match args[1] {
                    1 => Ok(state.pen),
                    2 => Ok(state.brush),
                    5 => Ok(state.palette),
                    6 => Ok(state.font),
                    7 => Ok(state.bitmap),
                    _ => Err(GdiError32::InvalidRequest),
                }
            }
            GetObjectType => self.model.object_type(args[0]),
            GetStockObject => crate::windows::gdi::GdiContext32::stock_object(args[0]),
            DeleteObject => self.delete_object(memory, args[0]),
            GetObjectA | GetObjectW => {
                if let Some(value) = self.font_object(memory, args, api == GetObjectW, ansi)? {
                    Ok(value)
                } else {
                    self.get_object(memory, args)
                }
            }
            CreateFontA | CreateFontW | CreateFontIndirectA | CreateFontIndirectW => {
                self.create_font(memory, api, args, ansi)
            }
            GetTextMetricsA
            | GetTextMetricsW
            | GetTextFaceA
            | GetTextFaceW
            | GetTextExtentPoint32A
            | GetTextExtentPoint32W
            | GetTextExtentExPointA
            | GetTextExtentExPointW
            | GetCharWidth32A
            | GetCharWidth32W
            | GetCharABCWidthsA
            | GetCharABCWidthsW
            | GetGlyphIndicesA
            | GetGlyphIndicesW => self.font_query(memory, api, args, ansi),
            DrawTextA | DrawTextW | DrawTextExA | DrawTextExW => {
                self.draw_text_call(memory, api, args, ansi)
            }
            TextOutA
            | TextOutW
            | ExtTextOutA
            | ExtTextOutW
            | GetTextAlign
            | SetTextAlign
            | GetTextCharacterExtra
            | SetTextCharacterExtra
            | SetTextJustification => self.text_call(memory, api, args, ansi),
            DrawEdge | DrawFrameControl => self.decoration_call(memory, api, args),
            ExtCreatePen => self.ext_create_pen(memory, args),
            CreatePen
            | CreatePenIndirect
            | CreateSolidBrush
            | CreateHatchBrush
            | CreatePatternBrush
            | CreateDIBPatternBrush
            | CreateDIBPatternBrushPt
            | CreateBrushIndirect => self.create_object(memory, api, args),
            CreateBitmap => {
                if args[2] != 1 || args[3] > u16::MAX as u32 {
                    return Err(GdiError32::Unsupported);
                }
                self.create_bitmap(
                    memory,
                    BitmapLayout32::new(args[0] as i32, args[1] as i32, args[3] as u16, false)?,
                    args[4],
                )
            }
            CreateBitmapIndirect => {
                let [_, width, height, _, format, bits] = memory::words(memory, args[0])?;
                if format & 0xffff != 1 {
                    return Err(GdiError32::Unsupported);
                }
                self.create_bitmap(
                    memory,
                    BitmapLayout32::new(width as i32, height as i32, (format >> 16) as u16, false)?,
                    bits,
                )
            }
            CreateCompatibleBitmap => {
                self.compatible_bitmap(memory, args[0], args[1] as i32, args[2] as i32)
            }
            CreateDIBSection => self.create_dib(memory, args),
            GetBitmapBits | SetBitmapBits => self.bitmap_bits(memory, api, args),
            GetDIBits | SetDIBits => self.dib_transfer(memory, api, args),
            GetDIBColorTable | SetDIBColorTable => self.dib_colors(memory, api, args),
            GdiFlush => Ok(1),
            AlphaBlend | TransparentBlt => self.msimg_bitmap_call(memory, api, args),
            GradientFill => self.gradient_call(memory, args),
            BitBlt | StretchBlt | PatBlt => self.blit(memory, api, args),
            StretchDIBits | SetDIBitsToDevice => self.draw_dib(memory, api, args),
            GetPixel | SetPixel | SetPixelV => self.pixel(memory, api, args),
            LineTo => self.line(memory, args),
            Rectangle | FillRect | FrameRect | InvertRect => self.rectangle(memory, api, args),
            SetTextColor | GetTextColor | SetBkColor | GetBkColor | SetBkMode | GetBkMode
            | SetDCBrushColor | GetDCBrushColor | SetDCPenColor | GetDCPenColor | SetROP2
            | GetROP2 | SetStretchBltMode | GetStretchBltMode | SetMapMode | GetMapMode => {
                self.scalar_state(api, args)
            }
            SetWindowOrgEx | GetWindowOrgEx | SetViewportOrgEx | GetViewportOrgEx
            | SetWindowExtEx | GetWindowExtEx | SetViewportExtEx | GetViewportExtEx
            | OffsetWindowOrgEx | OffsetViewportOrgEx | SetBrushOrgEx | GetBrushOrgEx
            | MoveToEx => self.point_state(memory, api, args),
            LPtoDP | DPtoLP => self.transform_points(memory, api, args),
            GetDeviceCaps => self.device_caps(args[0], args[1]),
            CreateRectRgn
            | CreateRectRgnIndirect
            | SetRectRgn
            | CombineRgn
            | OffsetRgn
            | EqualRgn
            | GetRgnBox
            | PtInRegion
            | RectInRegion
            | GetRegionData => self.region_call(memory, api, args),
            SelectClipRgn | ExtSelectClipRgn | IntersectClipRect | ExcludeClipRect | GetClipBox
            | GetClipRgn | PtVisible | RectVisible => self.clipping(memory, api, args),
            ReleaseDC => self
                .model
                .release_dc(args[0], args[1], thread)
                .map(u32::from),
            WindowFromDC => Ok(match self.model.dc_kind(args[0])? {
                DcKind32::Display { window, .. } => window,
                _ => 0,
            }),
            GetDC | GetDCEx | GetWindowDC | BeginPaint | EndPaint | InvalidateRect
            | ValidateRect | GetUpdateRect | UpdateWindow => Err(GdiError32::InvalidRequest),
        }
    }
}
