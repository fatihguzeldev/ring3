use super::super::Access;
use super::{API_BASE, GuestMemory, MemoryError, PAGE_SIZE, Permissions, desktop::DESKTOP, guest};

const OBJECT_BASE: u32 = API_BASE + 4096;
const ROOT: u32 = OBJECT_BASE;
const DEVICE: u32 = OBJECT_BASE + 4;
const ROOT_TABLE: u32 = OBJECT_BASE + 0x100;
const DEVICE_TABLE: u32 = OBJECT_BASE + 0x200;
const INVALID_CALL: u32 = 0x8876_086c;
const NOT_AVAILABLE: u32 = 0x8876_086a;
const MAX_PIXELS: u64 = 1_048_576;
const MAX_RECTS: u32 = 64;

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
    CreateDevice,
    Clear,
    Present,
    RootAddRef,
    RootRelease,
    DeviceAddRef,
    DeviceRelease,
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
            _ => return None,
        })
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::CreateDevice | Self::Clear => 7,
            Self::Present => 5,
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
        for (table, index, offset) in [
            (ROOT_TABLE, 1, 0x50),
            (ROOT_TABLE, 2, 0x54),
            (ROOT_TABLE, 15, 0x40),
            (DEVICE_TABLE, 1, 0x58),
            (DEVICE_TABLE, 2, 0x5c),
            (DEVICE_TABLE, 15, 0x48),
            (DEVICE_TABLE, 36, 0x44),
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
            Call::CreateDevice => return self.create_device(args, memory),
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
                        self.back = None;
                        self.root_refs = self.root_refs.saturating_sub(1);
                    }
                }
                self.device_refs
            }
            _ => INVALID_CALL,
        })
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
