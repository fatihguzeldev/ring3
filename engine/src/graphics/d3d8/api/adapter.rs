use super::HResult;
use crate::graphics::d3d8::resources::{Format, Usage};

pub const CAPS_WORDS: usize = 53;
pub const ADAPTER_IDENTIFIER_BYTES: usize = 1068;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode32 {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub format: Format,
}

impl DisplayMode32 {
    pub const fn words(self) -> [u32; 4] {
        [
            self.width,
            self.height,
            self.refresh_rate,
            self.format as u32,
        ]
    }
}

/// engine/backend contract, never a probe of the browser's physical adapter.
/// a host selects this profile before guest device creation and must implement
/// its semantic command stream, including the advertised sampling modes.
#[derive(Clone, Debug)]
pub struct AdapterProfile32 {
    pub display: DisplayMode32,
    pub modes: Vec<DisplayMode32>,
    pub max_texture_extent: u32,
    pub max_volume_extent: u32,
    pub max_anisotropy: u32,
    pub multisamples: u32,
    pub presentation_intervals: u32,
    pub read_scanline: bool,
}

impl AdapterProfile32 {
    pub fn validate(&self) -> Result<(), HResult> {
        if self.modes.is_empty()
            || self.modes.len() > 256
            || !self.modes.contains(&self.display)
            || !(1..=16_384).contains(&self.max_texture_extent)
            || !(1..=2048).contains(&self.max_volume_extent)
            || !(1..=16).contains(&self.max_anisotropy)
            || self.multisamples & 1 == 0
            || self.multisamples & !0x1_fffd != 0
            || self.presentation_intervals & 1 == 0
            || self.presentation_intervals & !0x8000_000f != 0
            || self.modes.iter().any(|mode| {
                mode.width == 0
                    || mode.height == 0
                    || mode.width > 16_384
                    || mode.height > 16_384
                    || mode.refresh_rate == 0
                    || !is_display_format(mode.format)
            })
        {
            Err(HResult::INVALID_CALL)
        } else {
            Ok(())
        }
    }
    pub fn new(display: DisplayMode32, modes: Vec<DisplayMode32>) -> Result<Self, HResult> {
        if modes.is_empty()
            || modes.len() > 256
            || !modes.contains(&display)
            || modes.iter().any(|mode| {
                mode.width == 0
                    || mode.height == 0
                    || mode.width > 16_384
                    || mode.height > 16_384
                    || mode.refresh_rate == 0
                    || !is_display_format(mode.format)
            })
        {
            return Err(HResult::INVALID_CALL);
        }
        Ok(Self {
            display,
            modes,
            max_texture_extent: 16_384,
            max_volume_extent: 2_048,
            max_anisotropy: 16,
            multisamples: 1,
            presentation_intervals: 0x8000_000f,
            read_scanline: false,
        })
    }

    pub fn check_device_type(
        &self,
        adapter: Format,
        target: Format,
        windowed: bool,
    ) -> Result<(), HResult> {
        if !is_display_format(adapter)
            || !is_render_format(target)
            || windowed && adapter != self.display.format
            || !compatible_display(adapter, target)
        {
            Err(HResult::NOT_AVAILABLE)
        } else {
            Ok(())
        }
    }

    pub fn check_format(
        &self,
        usage: Usage,
        resource_type: u32,
        format: Format,
    ) -> Result<(), HResult> {
        if usage.0 & !Usage::KNOWN != 0 || !(1..=5).contains(&resource_type) {
            return Err(HResult::INVALID_CALL);
        }
        if usage.0 & (Usage::RT_PATCHES | Usage::NP_PATCHES) != 0 {
            return Err(HResult::NOT_AVAILABLE);
        }
        let allowed = match resource_type {
            1 => 3,
            2 | 4 => Usage::DYNAMIC,
            3 | 5 => 3 | Usage::DYNAMIC,
            _ => 0,
        };
        if usage.0 & !allowed != 0 {
            return Err(HResult::NOT_AVAILABLE);
        }
        let available = match resource_type {
            1 => !format.is_buffer(),
            2 => !format.is_buffer() && !format.is_depth() && usage.0 & 3 == 0,
            3 | 5 => !format.is_buffer(),
            4 => !format.is_buffer() && !format.is_depth() && usage.0 & 3 == 0,
            6 => format == Format::VertexData && usage.0 & 3 == 0,
            7 => matches!(format, Format::Index16 | Format::Index32) && usage.0 & 3 == 0,
            _ => false,
        };
        if !available
            || usage.contains(Usage::RENDER_TARGET) && !is_render_format(format)
            || usage.contains(Usage::DEPTH_STENCIL) != format.is_depth()
            || usage.contains(Usage::RENDER_TARGET | Usage::DEPTH_STENCIL)
        {
            Err(HResult::NOT_AVAILABLE)
        } else {
            Ok(())
        }
    }

    pub fn check_multisample(&self, format: Format, sample: u32) -> Result<(), HResult> {
        if sample == 1 || sample > 16 {
            return Err(HResult::INVALID_CALL);
        }
        if !is_render_format(format) && !format.is_depth() || self.multisamples & (1 << sample) == 0
        {
            Err(HResult::NOT_AVAILABLE)
        } else {
            Ok(())
        }
    }

    pub fn check_depth_stencil(&self, color: Format, depth: Format) -> Result<(), HResult> {
        if is_render_format(color) && depth.is_depth() {
            Ok(())
        } else {
            Err(HResult::NOT_AVAILABLE)
        }
    }

    pub fn caps(&self) -> [u32; CAPS_WORDS] {
        let mut caps = [0; CAPS_WORDS];
        caps[0] = 1; // D3DDEVTYPE_HAL: commands execute through the selected host backend.
        caps[2] = if self.read_scanline { 0x20000 } else { 0 };
        caps[3] = 0x300a_0000; // windowed, managed/dynamic resources, fullscreen gamma.
        caps[4] = 0x20;
        caps[5] = self.presentation_intervals;
        caps[6] = 3;
        caps[7] = 0x0019_1ff0; // ordinary draw + hardware vertex processing; no patch caps.
        caps[8] = 0x0ff6;
        caps[9] = 0x0077_7199;
        caps[10] = 0xff;
        caps[11] = 0x1fff;
        caps[12] = 0x1fff;
        caps[13] = 0xff;
        caps[14] = 0x84208;
        caps[15] = 0x1ecc5; // cube/volume/mip, alpha, projected, unrestricted NPOT.
        caps[16] = 0x0703_0700;
        caps[17] = caps[16];
        caps[18] = caps[16];
        caps[19] = 0x3f;
        caps[20] = 0x3f;
        caps[21] = 0x1f;
        caps[22] = self.max_texture_extent;
        caps[23] = self.max_texture_extent;
        caps[24] = self.max_volume_extent;
        caps[25] = 32_768;
        caps[26] = self.max_texture_extent;
        caps[27] = self.max_anisotropy;
        caps[28] = 1.0e10f32.to_bits();
        for slot in [29, 30] {
            caps[slot] = (-32_768.0f32).to_bits();
        }
        for slot in [31, 32] {
            caps[slot] = 32_768.0f32.to_bits();
        }
        caps[34] = 0xff;
        caps[35] = 0x0018_0008;
        caps[36] = 0x03ff_ffff;
        caps[37] = 8;
        caps[38] = 8;
        caps[39] = 0x7b;
        caps[40] = 8;
        caps[41] = 6;
        caps[42] = 4;
        caps[43] = 255;
        caps[44] = 64.0f32.to_bits();
        caps[45] = 0x00ff_ffff;
        caps[46] = 0x00ff_ffff;
        caps[47] = 16;
        caps[48] = 255;
        caps[49] = 0xfffe_0101;
        caps[50] = 96;
        caps[51] = 0xffff_0104;
        caps[52] = 8.0f32.to_bits();
        caps
    }

    pub fn identifier(&self) -> [u8; ADAPTER_IDENTIFIER_BYTES] {
        let mut bytes = [0; ADAPTER_IDENTIFIER_BYTES];
        bytes[..11].copy_from_slice(b"ring3.d3d8\0");
        let description = b"Ring3 D3D8 semantic command backend\0";
        bytes[512..512 + description.len()].copy_from_slice(description);
        bytes[1024..1032].copy_from_slice(&0x0001_0000_0000_0000u64.to_le_bytes());
        bytes[1048..1064].copy_from_slice(b"Ring3D3D8Backend");
        bytes
    }
}

pub fn is_display_format(format: Format) -> bool {
    matches!(
        format,
        Format::R5G6B5 | Format::X1R5G5B5 | Format::X8R8G8B8 | Format::A2B10G10R10
    )
}

pub fn is_render_format(format: Format) -> bool {
    is_display_format(format)
        || matches!(
            format,
            Format::A8R8G8B8 | Format::A1R5G5B5 | Format::A4R4G4B4 | Format::X4R4G4B4
        )
}

pub(super) fn compatible_display(display: Format, target: Format) -> bool {
    display == target
        || matches!(
            (display, target),
            (Format::X8R8G8B8, Format::A8R8G8B8) | (Format::X1R5G5B5, Format::A1R5G5B5)
        )
}
