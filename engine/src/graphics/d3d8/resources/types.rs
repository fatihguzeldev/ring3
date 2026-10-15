use super::Format;
use crate::graphics::ResourceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError {
    InvalidCall,
    NotAvailable,
    StaleResource,
    ResourceCapacity,
    ByteCapacity,
    Allocation,
    IdentityExhausted,
    ReadbackRequired,
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Pool {
    Default = 0,
    Managed = 1,
    SystemMemory = 2,
    Scratch = 3,
}

impl TryFrom<u32> for Pool {
    type Error = ResourceError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Default),
            1 => Ok(Self::Managed),
            2 => Ok(Self::SystemMemory),
            3 => Ok(Self::Scratch),
            _ => Err(ResourceError::InvalidCall),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage(pub u32);

impl Usage {
    pub const RENDER_TARGET: u32 = 0x1;
    pub const DEPTH_STENCIL: u32 = 0x2;
    pub const WRITE_ONLY: u32 = 0x8;
    pub const SOFTWARE_PROCESSING: u32 = 0x10;
    pub const DO_NOT_CLIP: u32 = 0x20;
    pub const POINTS: u32 = 0x40;
    pub const RT_PATCHES: u32 = 0x80;
    pub const NP_PATCHES: u32 = 0x100;
    pub const DYNAMIC: u32 = 0x200;
    pub const KNOWN: u32 = 0x3fb;

    pub const fn contains(self, flags: u32) -> bool {
        self.0 & flags == flags
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Vertex { fvf: u32 },
    Index { format: Format },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    pub length: u32,
    pub kind: BufferKind,
    pub usage: Usage,
    pub pool: Pool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TextureKind {
    Texture2D = 3,
    Volume = 4,
    Cube = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureDesc {
    pub kind: TextureKind,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub levels: u32,
    pub format: Format,
    pub usage: Usage,
    pub pool: Pool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDesc {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub usage: Usage,
    pub pool: Pool,
    pub multisample: u32,
    pub lockable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceDesc {
    Buffer(BufferDesc),
    Texture(TextureDesc),
    Surface(SurfaceDesc),
}

impl ResourceDesc {
    pub const fn pool(self) -> Pool {
        match self {
            Self::Buffer(desc) => desc.pool,
            Self::Texture(desc) => desc.pool,
            Self::Surface(desc) => desc.pool,
        }
    }

    pub const fn usage(self) -> Usage {
        match self {
            Self::Buffer(desc) => desc.usage,
            Self::Texture(desc) => desc.usage,
            Self::Surface(desc) => desc.usage,
        }
    }

    pub const fn resource_type(self) -> u32 {
        match self {
            Self::Surface(_) => 1,
            Self::Texture(desc) => desc.kind as u32,
            Self::Buffer(BufferDesc {
                kind: BufferKind::Vertex { .. },
                ..
            }) => 6,
            Self::Buffer(BufferDesc {
                kind: BufferKind::Index { .. },
                ..
            }) => 7,
        }
    }

    pub const fn format(self) -> Format {
        match self {
            Self::Surface(desc) => desc.format,
            Self::Texture(desc) => desc.format,
            Self::Buffer(BufferDesc {
                kind: BufferKind::Index { format },
                ..
            }) => format,
            Self::Buffer(_) => Format::VertexData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceRef {
    pub resource: ResourceId,
    pub subresource: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubresourceInfo {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub row_pitch: u32,
    pub slice_pitch: u32,
    pub length: u32,
    pub face: u32,
    pub level: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box3 {
    pub left: u32,
    pub top: u32,
    pub front: u32,
    pub right: u32,
    pub bottom: u32,
    pub back: u32,
}

impl From<Rect> for Box3 {
    fn from(rect: Rect) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            front: 0,
            right: rect.right,
            bottom: rect.bottom,
            back: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockFlags(pub u32);

impl LockFlags {
    pub const READ_ONLY: u32 = 0x10;
    pub const NO_SYSTEM_LOCK: u32 = 0x800;
    pub const NO_OVERWRITE: u32 = 0x1000;
    pub const DISCARD: u32 = 0x2000;
    pub const NO_DIRTY_UPDATE: u32 = 0x8000;
    pub const KNOWN: u32 = 0xb810;

    pub const fn contains(self, flags: u32) -> bool {
        self.0 & flags == flags
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockInfo {
    pub token: u64,
    pub subresource: u32,
    pub offset: u32,
    pub row_pitch: u32,
    pub slice_pitch: u32,
    pub length: u32,
    pub flags: LockFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlockPlan {
    pub info: LockInfo,
    pub version: Option<u64>,
    pub row_bytes: u32,
    pub rows: u32,
    pub slices: u32,
}
