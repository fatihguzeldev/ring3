use super::{AdapterProfile32, DisplayMode32, HResult};
use crate::graphics::d3d8::resources::Format;
use crate::windows::window::WindowExtent32;

pub const SDK_VERSION: u32 = 220;
pub const PRESENT_PARAMETER_WORDS: usize = 13;

/// the desktop D3D8 layout. d3d9's additional fields are deliberately absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationParameters32 {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub back_buffers: u32,
    pub multisample: u32,
    pub swap_effect: u32,
    pub window: u32,
    pub windowed: bool,
    pub auto_depth: bool,
    pub depth_format: u32,
    pub flags: u32,
    pub refresh_rate: u32,
    pub interval: u32,
}

impl PresentationParameters32 {
    pub fn decode(words: [u32; PRESENT_PARAMETER_WORDS]) -> Self {
        Self {
            width: words[0],
            height: words[1],
            format: words[2],
            back_buffers: words[3],
            multisample: words[4],
            swap_effect: words[5],
            window: words[6],
            windowed: words[7] != 0,
            auto_depth: words[8] != 0,
            depth_format: words[9],
            flags: words[10],
            refresh_rate: words[11],
            interval: words[12],
        }
    }

    pub fn words(self) -> [u32; PRESENT_PARAMETER_WORDS] {
        [
            self.width,
            self.height,
            self.format,
            self.back_buffers,
            self.multisample,
            self.swap_effect,
            self.window,
            u32::from(self.windowed),
            u32::from(self.auto_depth),
            self.depth_format,
            self.flags,
            self.refresh_rate,
            self.interval,
        ]
    }

    /// validate and normalize without publishing any guest memory or resources.
    pub fn normalize(
        mut self,
        focus_window: u32,
        extent: WindowExtent32,
        adapter: &AdapterProfile32,
        additional: bool,
    ) -> Result<Self, HResult> {
        if self.window == 0 {
            self.window = focus_window;
        }
        if self.window == 0
            || additional && !self.windowed
            || !(1..=4).contains(&self.swap_effect)
            || self.flags & !1 != 0
        {
            return Err(HResult::INVALID_CALL);
        }
        if self.windowed {
            if self.width == 0 {
                self.width = extent.width;
            }
            if self.height == 0 {
                self.height = extent.height;
            }
            if self.format == 0 {
                self.format = adapter.display.format as u32;
            }
            if self.refresh_rate != 0 || self.interval != 0 {
                return Err(HResult::INVALID_CALL);
            }
        }
        if self.back_buffers == 0 {
            self.back_buffers = 1;
        }
        if self.back_buffers > 3
            || matches!(self.swap_effect, 3 | 4) && self.back_buffers != 1
            || self.multisample != 0 && self.swap_effect != 1
            || self.multisample != 0 && self.flags & 1 != 0
            || self.width == 0
            || self.height == 0
            || self.width > adapter.max_texture_extent
            || self.height > adapter.max_texture_extent
        {
            return Err(HResult::INVALID_CALL);
        }
        let format = Format::try_from(self.format).map_err(|_| HResult::NOT_AVAILABLE)?;
        let mode = self.display_mode(adapter)?;
        adapter.check_device_type(mode.format, format, self.windowed)?;
        adapter.check_multisample(format, self.multisample)?;
        if self.auto_depth {
            let depth = Format::try_from(self.depth_format).map_err(|_| HResult::NOT_AVAILABLE)?;
            adapter.check_depth_stencil(format, depth)?;
            adapter.check_multisample(depth, self.multisample)?;
        }
        if !self.windowed
            && self.interval != 0
            && (self.interval.count_ones() != 1
                || self.interval & adapter.presentation_intervals == 0)
        {
            return Err(HResult::INVALID_CALL);
        }
        Ok(self)
    }

    pub fn display_mode(self, adapter: &AdapterProfile32) -> Result<DisplayMode32, HResult> {
        if self.windowed {
            return Ok(adapter.display);
        }
        let format = Format::try_from(self.format).map_err(|_| HResult::NOT_AVAILABLE)?;
        let matches = |mode: &&DisplayMode32| {
            mode.width == self.width
                && mode.height == self.height
                && super::adapter::compatible_display(mode.format, format)
                && (matches!(self.refresh_rate, 0 | 0x7fff_ffff)
                    || self.refresh_rate == mode.refresh_rate)
        };
        adapter
            .modes
            .iter()
            .filter(matches)
            .find(|mode| mode.refresh_rate == adapter.display.refresh_rate)
            .or_else(|| adapter.modes.iter().find(matches))
            .copied()
            .ok_or(HResult::NOT_AVAILABLE)
    }
}

pub fn validate_behavior(flags: u32) -> Result<(), HResult> {
    let processing = flags & 0xe0;
    if flags & !0xf6 != 0
        || !matches!(processing, 0x20 | 0x40 | 0x80)
        || flags & 0x10 != 0 && processing != 0x40
    {
        Err(HResult::INVALID_CALL)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
