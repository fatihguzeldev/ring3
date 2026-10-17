use crate::windows::user32::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum ImageKind32 {
    Bitmap = 0,
    Icon = 1,
    Cursor = 2,
}
impl TryFrom<u32> for ImageKind32 {
    type Error = E;
    fn try_from(value: u32) -> Result<Self, E> {
        match value {
            0 => Ok(Self::Bitmap),
            1 => Ok(Self::Icon),
            2 => Ok(Self::Cursor),
            _ => Err(E::Win32(87)),
        }
    }
}
impl ImageKind32 {
    pub(crate) fn group_type(self) -> u16 {
        match self {
            Self::Icon => 14,
            Self::Cursor => 12,
            Self::Bitmap => 2,
        }
    }
    pub(crate) fn resource_type(self) -> u16 {
        match self {
            Self::Icon => 3,
            Self::Cursor => 1,
            Self::Bitmap => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageExtent32 {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoadImage32 {
    pub kind: ImageKind32,
    pub flags: u32,
    pub width: u32,
    pub height: u32,
}
impl LoadImage32 {
    pub(crate) fn new(kind: ImageKind32, width: i32, height: i32, flags: u32) -> Result<Self, E> {
        if width < 0 || height < 0 || flags & !0xb0f1 != 0 {
            return Err(E::Win32(87));
        }
        Ok(Self {
            kind,
            flags,
            width: width as u32,
            height: height as u32,
        })
    }
    pub(crate) fn shared(self) -> bool {
        self.flags & 0x8000 != 0
    }
    pub(crate) fn from_file(self) -> bool {
        self.flags & 0x10 != 0
    }
    pub(crate) fn monochrome(self) -> bool {
        self.flags & 1 != 0
    }
    pub(crate) fn dib_section(self) -> bool {
        self.kind == ImageKind32::Bitmap && self.flags & 0x2000 != 0
    }
    pub(crate) fn extent(self, source: ImageExtent32, defaults: ImageExtent32) -> ImageExtent32 {
        extent(
            self.width,
            self.height,
            self.flags,
            source,
            if self.kind == ImageKind32::Bitmap {
                source
            } else {
                defaults
            },
        )
    }
    pub(crate) fn requested_extent(self, defaults: ImageExtent32) -> ImageExtent32 {
        let use_defaults = self.kind != ImageKind32::Bitmap && self.flags & 0x40 != 0;
        ImageExtent32 {
            width: if self.width == 0 && use_defaults {
                defaults.width
            } else {
                self.width
            },
            height: if self.height == 0 && use_defaults {
                defaults.height
            } else {
                self.height
            },
        }
    }
    pub(crate) fn selection(
        self,
        first: ImageExtent32,
        defaults: ImageExtent32,
    ) -> Option<ImageExtent32> {
        (self.width != 0 || self.height != 0 || self.flags & 0x40 != 0)
            .then(|| self.extent(first, defaults))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CopyImage32 {
    pub kind: ImageKind32,
    pub flags: u32,
    pub width: u32,
    pub height: u32,
}
impl CopyImage32 {
    pub(crate) fn new(kind: ImageKind32, width: i32, height: i32, flags: u32) -> Result<Self, E> {
        if width < 0
            || height < 0
            || flags & !0x604d != 0
            || (kind != ImageKind32::Bitmap && flags & 0x2000 != 0)
        {
            return Err(E::Win32(87));
        }
        Ok(Self {
            kind,
            flags,
            width: width as u32,
            height: height as u32,
        })
    }
    pub(crate) fn return_original(self) -> bool {
        self.flags & 4 != 0
    }
    pub(crate) fn delete_original(self) -> bool {
        self.flags & 8 != 0
    }
    pub(crate) fn from_resource(self) -> bool {
        self.flags & 0x4000 != 0
    }
    pub(crate) fn extent(self, source: ImageExtent32, defaults: ImageExtent32) -> ImageExtent32 {
        extent(
            self.width,
            self.height,
            self.flags,
            source,
            if self.kind == ImageKind32::Bitmap {
                source
            } else {
                defaults
            },
        )
    }
}

fn extent(
    width: u32,
    height: u32,
    flags: u32,
    source: ImageExtent32,
    defaults: ImageExtent32,
) -> ImageExtent32 {
    let zero = if flags & 0x40 == 0 { source } else { defaults };
    ImageExtent32 {
        width: if width == 0 { zero.width } else { width },
        height: if height == 0 { zero.height } else { height },
    }
}
