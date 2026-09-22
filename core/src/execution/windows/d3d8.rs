use std::collections::BTreeMap;

use super::super::Access;
use super::{API_BASE, GuestMemory, MemoryError, PAGE_SIZE, Permissions, desktop::DESKTOP, guest};

const OBJECT_BASE: u32 = API_BASE + 4096;
const ROOT: u32 = OBJECT_BASE;
const DEVICE: u32 = OBJECT_BASE + 4;
const ROOT_TABLE: u32 = OBJECT_BASE + 0x100;
const DEVICE_TABLE: u32 = OBJECT_BASE + 0x200;
const INVALID_CALL: u32 = 0x8876_086c;
const NOT_AVAILABLE: u32 = 0x8876_086a;
const ADAPTER_IDENTIFIER_SIZE: usize = 1068;
const ADAPTER_DRIVER: &[u8] = b"ring3\0";
const ADAPTER_DESCRIPTION: &[u8] = b"Ring3 Virtual Display Adapter\0";
const DEVICE_CAPS_SIZE: usize = 212;
const CAPS2_CAN_RENDER_WINDOWED: u32 = 0x0008_0000;
const MAX_PIXELS: u64 = 1_048_576;
const MAX_RECTS: u32 = 64;
const TEXTURE_TABLE: u32 = OBJECT_BASE + 0x400;
const TEXTURE_START: u64 = 0x7200_0000;
const TEXTURE_END: u64 = 0x7f00_0000;
const OUT_OF_VIDEO_MEMORY: u32 = 0x8876_017c;

/// an owned top-to-bottom rgba8 snapshot of the most recent presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Create,
    AdapterCount,
    AdapterIdentifier,
    AdapterModeCount,
    AdapterMode,
    CheckDeviceType,
    CheckDeviceFormat,
    CheckMultiSampleType,
    DeviceCaps,
    CreateDevice,
    CreateTexture,
    Clear,
    Present,
    RootAddRef,
    RootRelease,
    DeviceAddRef,
    DeviceRelease,
    TextureAddRef,
    TextureRelease,
    TextureLevelCount,
    TextureLevelDesc,
    TextureLockRect,
    TextureUnlockRect,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        Some(match offset {
            12 => Self::Create,
            0x40 => Self::CreateDevice,
            0x44 => Self::Clear,
            0x48 => Self::Present,
            0x50 => Self::RootAddRef,
            0x54 => Self::RootRelease,
            0x58 => Self::DeviceAddRef,
            0x5c => Self::DeviceRelease,
            0x400 => Self::CreateTexture,
            0x404 => Self::TextureAddRef,
            0x408 => Self::TextureRelease,
            0x40c => Self::TextureLevelCount,
            0x410 => Self::TextureLevelDesc,
            0x414 => Self::TextureLockRect,
            0x418 => Self::TextureUnlockRect,
            0x2d0 => Self::AdapterCount,
            0x2d4 => Self::AdapterIdentifier,
            0x2d8 => Self::DeviceCaps,
            0x2dc => Self::AdapterModeCount,
            0x2e0 => Self::AdapterMode,
            0x2e4 => Self::CheckDeviceType,
            0x2e8 => Self::CheckDeviceFormat,
            0x2ec => Self::CheckMultiSampleType,
            _ => return None,
        })
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::CreateDevice | Self::Clear | Self::CheckDeviceFormat => 7,
            Self::CreateTexture => 8,
            Self::Present | Self::TextureLockRect => 5,
            Self::TextureLevelDesc => 3,
            Self::AdapterIdentifier | Self::AdapterMode | Self::DeviceCaps => 4,
            Self::AdapterModeCount | Self::TextureUnlockRect => 2,
            Self::CheckDeviceType | Self::CheckMultiSampleType => 6,
            _ => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Graphics {
    root_refs: u32,
    device_refs: u32,
    back: Option<Frame>,
    front: Option<Frame>,
    textures: BTreeMap<u32, Texture>,
}

struct Texture {
    refs: u32,
    length: u64,
    format: u32,
    pool: u32,
    levels: Vec<TextureLevel>,
}

struct TextureLevel {
    width: u32,
    height: u32,
    offset: u32,
    locked: bool,
}

impl Graphics {
    pub(super) fn initialize(memory: &mut GuestMemory) -> Result<(), MemoryError> {
        memory.map_zeroed(u64::from(OBJECT_BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
        guest::write_word(memory, ROOT, ROOT_TABLE)?;
        guest::write_word(memory, DEVICE, DEVICE_TABLE)?;
        for (table, count) in [(ROOT_TABLE, 16), (DEVICE_TABLE, 97)] {
            for index in 0..count {
                guest::write_word(memory, table + index * 4, API_BASE + 0xffc)?;
            }
        }
        for index in 0..19 {
            guest::write_word(memory, TEXTURE_TABLE + index * 4, API_BASE + 0xffc)?;
        }
        for (table, index, offset) in [
            (ROOT_TABLE, 1, 0x50),
            (ROOT_TABLE, 2, 0x54),
            (ROOT_TABLE, 4, 0x2d0),
            (ROOT_TABLE, 5, 0x2d4),
            (ROOT_TABLE, 6, 0x2dc),
            (ROOT_TABLE, 7, 0x2e0),
            (ROOT_TABLE, 9, 0x2e4),
            (ROOT_TABLE, 10, 0x2e8),
            (ROOT_TABLE, 11, 0x2ec),
            (ROOT_TABLE, 13, 0x2d8),
            (ROOT_TABLE, 15, 0x40),
            (DEVICE_TABLE, 1, 0x58),
            (DEVICE_TABLE, 2, 0x5c),
            (DEVICE_TABLE, 15, 0x48),
            (DEVICE_TABLE, 36, 0x44),
            (DEVICE_TABLE, 20, 0x400),
            (TEXTURE_TABLE, 1, 0x404),
            (TEXTURE_TABLE, 2, 0x408),
            (TEXTURE_TABLE, 13, 0x40c),
            (TEXTURE_TABLE, 14, 0x410),
            (TEXTURE_TABLE, 16, 0x414),
            (TEXTURE_TABLE, 17, 0x418),
        ] {
            guest::write_word(memory, table + index * 4, API_BASE + offset)?;
        }
        memory.protect(u64::from(OBJECT_BASE), PAGE_SIZE, Permissions::READ)
    }

    pub(super) fn take_frame(&mut self) -> Option<Frame> {
        self.front.take()
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        Ok(match call {
            Call::Create => {
                if !matches!(args[0], 120 | 220) || self.root_refs != 0 {
                    0
                } else {
                    self.root_refs = 1;
                    ROOT
                }
            }
            Call::AdapterCount => u32::from(args[0] == ROOT && self.root_refs != 0),
            Call::AdapterIdentifier => return self.adapter_identifier(args, memory),
            Call::AdapterModeCount => {
                u32::from(args[0] == ROOT && self.root_refs != 0 && args[1] == 0)
            }
            Call::AdapterMode => return self.adapter_mode(args, memory),
            Call::CheckDeviceType => self.check_device_type(args),
            Call::CheckDeviceFormat => self.check_device_format(args),
            Call::CheckMultiSampleType => self.check_multisample_type(args),
            Call::DeviceCaps => return self.device_caps(args, memory),
            Call::CreateDevice => return self.create_device(args, memory),
            Call::CreateTexture => return self.create_texture(args, memory),
            Call::TextureLevelDesc => return self.texture_level_desc(args, memory),
            Call::TextureLockRect => return self.texture_lock_rect(args, memory),
            Call::TextureUnlockRect => self.texture_unlock_rect(args),
            Call::TextureLevelCount => {
                self.textures.get(&args[0]).map_or(INVALID_CALL, |texture| {
                    u32::try_from(texture.levels.len()).expect("bounded mip count")
                })
            }
            Call::TextureAddRef => {
                if let Some(texture) = self.textures.get_mut(&args[0]) {
                    texture.refs = texture.refs.saturating_add(1);
                    texture.refs
                } else {
                    INVALID_CALL
                }
            }
            Call::TextureRelease => return self.texture_release(args[0], memory),
            Call::Clear => return self.clear(args, memory),
            Call::Present => {
                if args[0] != DEVICE || self.device_refs == 0 || args[1..5] != [0; 4] {
                    INVALID_CALL
                } else {
                    self.front.clone_from(&self.back);
                    0
                }
            }
            Call::RootAddRef | Call::RootRelease if args[0] == ROOT && self.root_refs != 0 => {
                if matches!(call, Call::RootAddRef) {
                    self.root_refs = self.root_refs.saturating_add(1);
                } else {
                    self.root_refs -= 1;
                }
                self.root_refs
            }
            Call::DeviceAddRef | Call::DeviceRelease
                if args[0] == DEVICE && self.device_refs != 0 =>
            {
                if matches!(call, Call::DeviceAddRef) {
                    self.device_refs = self.device_refs.saturating_add(1);
                } else {
                    self.device_refs -= 1;
                    if self.device_refs == 0 {
                        self.finish_device();
                    }
                }
                self.device_refs
            }
            _ => INVALID_CALL,
        })
    }

    fn adapter_identifier(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 0 | 2) {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[3], ADAPTER_IDENTIFIER_SIZE, Access::Write)?;
        let mut identifier = [0; ADAPTER_IDENTIFIER_SIZE];
        identifier[..ADAPTER_DRIVER.len()].copy_from_slice(ADAPTER_DRIVER);
        let description = &mut identifier[512..];
        description[..ADAPTER_DESCRIPTION.len()].copy_from_slice(ADAPTER_DESCRIPTION);
        memory.write(u64::from(args[3]), &identifier)?;
        Ok(0)
    }

    fn device_caps(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return Ok(INVALID_CALL);
        }
        if args[2] != 1 {
            return Ok(NOT_AVAILABLE);
        }
        guest::check(memory, args[3], DEVICE_CAPS_SIZE, Access::Write)?;
        let mut caps = [0; DEVICE_CAPS_SIZE];
        caps[..4].copy_from_slice(&1_u32.to_le_bytes());
        caps[12..16].copy_from_slice(&CAPS2_CAN_RENDER_WINDOWED.to_le_bytes());
        memory.write(u64::from(args[3]), &caps)?;
        Ok(0)
    }

    fn adapter_mode(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || args[2] != 0 {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[3], 16, Access::Write)?;
        let mut mode = [0; 16];
        for (field, value) in mode.chunks_exact_mut(4).zip([640_u32, 480, 0, 22]) {
            field.copy_from_slice(&value.to_le_bytes());
        }
        memory.write(u64::from(args[3]), &mode)?;
        Ok(0)
    }

    fn check_device_type(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return INVALID_CALL;
        }
        if args[2] == 1 && args[3] == 22 && args[4] == 22 {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn check_device_format(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return INVALID_CALL;
        }
        if matches!(args[2..], [1, 22, 1, 1, 22] | [1, 22, 0, 3, 21 | 22]) {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn check_multisample_type(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT
            || self.root_refs == 0
            || args[1] != 0
            || !matches!(args[2], 1..=3)
            || args[5] > 16
        {
            return INVALID_CALL;
        }
        if args[2] == 1 && args[3] == 22 && args[5] == 0 {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn create_device(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        if args[0] != ROOT
            || self.root_refs == 0
            || args[1] != 0
            || args[2] != 1
            || !matches!(args[3], 0 | DESKTOP)
            || args[4] != 0x20
        {
            return Ok(INVALID_CALL);
        }
        if self.device_refs != 0 {
            return Ok(NOT_AVAILABLE);
        }
        let mut params = [0; 13];
        guest::read_words(memory, args[5], &mut params)?;
        let [
            width,
            height,
            format,
            count,
            samples,
            swap,
            window,
            windowed,
            depth,
            _,
            flags,
            refresh,
            interval,
        ] = params;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || format != 22
            || count > 1
            || samples != 0
            || swap != 1
            || !matches!(window, 0 | DESKTOP)
            || (window == 0 && args[3] == 0)
            || windowed == 0
            || depth != 0
            || flags != 0
            || refresh != 0
            || interval != 0
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[5], 52, Access::Write)?;
        guest::check(memory, args[6], 4, Access::Write)?;
        let mut frame = Frame {
            width,
            height,
            rgba: vec![
                0;
                usize::try_from(u64::from(width) * u64::from(height) * 4)
                    .expect("admitted frame fits a 32-bit host")
            ],
        };
        for pixel in frame.rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        guest::write_word(memory, args[5] + 12, 1)?;
        guest::write_word(memory, args[6], DEVICE)?;
        self.root_refs = self.root_refs.saturating_add(1);
        self.device_refs = 1;
        self.back = Some(frame);
        Ok(0)
    }

    fn finish_device(&mut self) {
        self.back = None;
        self.root_refs = self.root_refs.saturating_sub(1);
    }

    fn create_texture(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [
            device,
            width,
            height,
            requested_levels,
            usage,
            format,
            pool,
            output,
        ] = <[u32; 8]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE
            || self.device_refs == 0
            || width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || usage != 0
            || !matches!(format, 21 | 22)
            || !matches!(pool, 0..=2)
        {
            return Ok(INVALID_CALL);
        }

        let mut levels = Vec::new();
        let (mut level_width, mut level_height) = (width, height);
        let mut pixel_bytes = 0_u64;
        loop {
            levels.push(TextureLevel {
                width: level_width,
                height: level_height,
                offset: u32::try_from(PAGE_SIZE + pixel_bytes)
                    .expect("bounded texture address offset"),
                locked: false,
            });
            pixel_bytes += u64::from(level_width) * u64::from(level_height) * 4;
            if (requested_levels != 0 && levels.len() == requested_levels as usize)
                || (level_width == 1 && level_height == 1)
            {
                break;
            }
            level_width = (level_width / 2).max(1);
            level_height = (level_height / 2).max(1);
        }
        if requested_levels != 0 && levels.len() != requested_levels as usize {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        let length = PAGE_SIZE + pixel_bytes.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let Some(address) = memory.first_free_span(TEXTURE_START, TEXTURE_END, length)? else {
            return Ok(OUT_OF_VIDEO_MEMORY);
        };
        if memory
            .map_zeroed(address, length, Permissions::READ_WRITE)
            .is_err()
        {
            return Ok(OUT_OF_VIDEO_MEMORY);
        }
        let address = u32::try_from(address).expect("texture guest range is 32-bit");
        guest::write_word(memory, address, TEXTURE_TABLE)?;
        memory.protect(u64::from(address), PAGE_SIZE, Permissions::READ)?;
        guest::write_word(memory, output, address)?;
        self.textures.insert(
            address,
            Texture {
                refs: 1,
                length,
                format,
                pool,
                levels,
            },
        );
        self.device_refs = self.device_refs.saturating_add(1);
        Ok(0)
    }

    fn texture_release(
        &mut self,
        address: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get_mut(&address) else {
            return Ok(INVALID_CALL);
        };
        if texture.refs > 1 {
            texture.refs -= 1;
            return Ok(texture.refs);
        }
        memory.unmap(u64::from(address), texture.length)?;
        self.textures.remove(&address);
        self.device_refs -= 1;
        if self.device_refs == 0 {
            self.finish_device();
        }
        Ok(0)
    }

    fn texture_level_desc(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get(&args[0]) else {
            return Ok(INVALID_CALL);
        };
        let Some(level) = texture.levels.get(args[1] as usize) else {
            return Ok(INVALID_CALL);
        };
        guest::check(memory, args[2], 32, Access::Write)?;
        let values = [
            texture.format,
            3,
            0,
            texture.pool,
            level.width * level.height * 4,
            0,
            level.width,
            level.height,
        ];
        let bytes: Vec<_> = values.into_iter().flat_map(u32::to_le_bytes).collect();
        memory.write(u64::from(args[2]), &bytes)?;
        Ok(0)
    }

    fn texture_lock_rect(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get(&args[0]) else {
            return Ok(INVALID_CALL);
        };
        let Some(level) = texture.levels.get(args[1] as usize) else {
            return Ok(INVALID_CALL);
        };
        if level.locked || args[4] != 0 {
            return Ok(INVALID_CALL);
        }
        let (left, top) = if args[3] == 0 {
            (0, 0)
        } else {
            let mut rect = [0; 4];
            guest::read_words(memory, args[3], &mut rect)?;
            if rect[0] >= rect[2]
                || rect[1] >= rect[3]
                || rect[2] > level.width
                || rect[3] > level.height
            {
                return Ok(INVALID_CALL);
            }
            (rect[0], rect[1])
        };
        guest::check(memory, args[2], 8, Access::Write)?;
        let pitch = level.width * 4;
        let pixels = args[0] + level.offset + top * pitch + left * 4;
        let bytes: Vec<_> = [pitch, pixels]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        memory.write(u64::from(args[2]), &bytes)?;
        self.textures.get_mut(&args[0]).unwrap().levels[args[1] as usize].locked = true;
        Ok(0)
    }

    fn texture_unlock_rect(&mut self, args: &[u32]) -> u32 {
        let Some(level) = self
            .textures
            .get_mut(&args[0])
            .and_then(|texture| texture.levels.get_mut(args[1] as usize))
        else {
            return INVALID_CALL;
        };
        if !level.locked {
            return INVALID_CALL;
        }
        level.locked = false;
        0
    }

    fn clear(&mut self, args: &[u32], memory: &GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != DEVICE
            || self.device_refs == 0
            || args[3] != 1
            || args[1] > MAX_RECTS
            || (args[1] == 0) != (args[2] == 0)
        {
            return Ok(INVALID_CALL);
        }
        let Some(back) = self.back.as_mut() else {
            return Ok(INVALID_CALL);
        };
        let mut rects = vec![[0; 4]; args[1] as usize];
        guest::check(memory, args[2], rects.len() * 16, Access::Read)?;
        for (index, rect) in (0_u32..).zip(&mut rects) {
            guest::read_words(memory, args[2] + index * 16, rect)?;
            if rect[0].cast_signed() > rect[2].cast_signed()
                || rect[1].cast_signed() > rect[3].cast_signed()
            {
                return Ok(INVALID_CALL);
            }
        }
        if rects.is_empty() {
            rects.push([0, 0, back.width, back.height]);
        }
        let [_, red, green, blue] = args[4].to_be_bytes();
        for rect in rects {
            let x1 = rect[0].cast_signed().max(0).cast_unsigned().min(back.width);
            let y1 = rect[1]
                .cast_signed()
                .max(0)
                .cast_unsigned()
                .min(back.height);
            let x2 = rect[2].cast_signed().max(0).cast_unsigned().min(back.width);
            let y2 = rect[3]
                .cast_signed()
                .max(0)
                .cast_unsigned()
                .min(back.height);
            for y in y1..y2 {
                let start = ((y * back.width + x1) * 4) as usize;
                let end = ((y * back.width + x2) * 4) as usize;
                for pixel in back.rgba[start..end].chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[red, green, blue, 255]);
                }
            }
        }
        Ok(0)
    }
}
