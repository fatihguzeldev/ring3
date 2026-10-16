#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenFileId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLimits32 {
    pub nodes: usize,
    pub opens: usize,
    pub cache_pages: usize,
    pub overlay_pages: usize,
    pub requests: usize,
    pub max_io_bytes: usize,
}
impl Default for FileLimits32 {
    fn default() -> Self {
        Self {
            nodes: 16_384,
            opens: 4096,
            cache_pages: 256,
            overlay_pages: 1024,
            requests: 64,
            max_io_bytes: 16 * 1024 * 1024,
        }
    }
}
pub const FILE_PAGE_BYTES: usize = 65_536;
pub const ATTR_READONLY: u32 = 1;
pub const ATTR_HIDDEN: u32 = 2;
pub const ATTR_SYSTEM: u32 = 4;
pub const ATTR_DIRECTORY: u32 = 0x10;
pub const ATTR_ARCHIVE: u32 = 0x20;
pub const ATTR_NORMAL: u32 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError32 {
    InvalidLimits,
    InvalidHandle,
    NoAccess,
    InvalidAddress,
    MappedAlignment,
    FileInvalid,
    LockViolation,
    UserMappedFile,
    InvalidParameter,
    InvalidName,
    AccessDenied,
    NotFound,
    PathNotFound,
    SharingViolation,
    AlreadyExists,
    FileExists,
    DeletePending,
    DirectoryNotEmpty,
    NotDirectory,
    NoMoreFiles,
    Capacity,
    Allocation,
    Stale,
    Busy,
    Unsupported,
    DiskFull,
    NegativeSeek,
    NotSameDevice,
    Exhausted,
}
impl FileError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::LockViolation => 33,
            Self::UserMappedFile => 1224,
            Self::InvalidAddress => 487,
            Self::MappedAlignment => 1132,
            Self::FileInvalid => 1006,
            Self::InvalidHandle | Self::Stale => 6,
            Self::NoAccess => 998,
            Self::InvalidLimits | Self::InvalidParameter => 87,
            Self::InvalidName => 123,
            Self::AccessDenied | Self::DeletePending => 5,
            Self::NotFound => 2,
            Self::PathNotFound => 3,
            Self::SharingViolation => 32,
            Self::AlreadyExists => 183,
            Self::FileExists => 80,
            Self::DirectoryNotEmpty => 145,
            Self::NotDirectory => 267,
            Self::NoMoreFiles => 18,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Busy => 170,
            Self::Unsupported => 50,
            Self::DiskFull => 112,
            Self::NegativeSeek => 131,
            Self::NotSameDevice => 17,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileAccess32(pub u32);
impl FileAccess32 {
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const APPEND: Self = Self(4);
    pub const DELETE: Self = Self(8);
    pub const READ_ATTRIBUTES: Self = Self(16);
    pub const WRITE_ATTRIBUTES: Self = Self(32);
    pub const EXECUTE: Self = Self(64);
    pub const ALL: Self = Self(127);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileShare32(pub u32);
impl FileShare32 {
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const DELETE: Self = Self(4);
    pub const ALL: Self = Self(7);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDisposition32 {
    CreateNew,
    CreateAlways,
    OpenExisting,
    OpenAlways,
    TruncateExisting,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileOpenOptions32 {
    pub access: FileAccess32,
    pub share: FileShare32,
    pub disposition: FileDisposition32,
    pub attributes: u32,
    pub directory: bool,
    pub delete_on_close: bool,
    pub overlapped: bool,
    pub no_buffering: bool,
}
impl Default for FileOpenOptions32 {
    fn default() -> Self {
        Self {
            access: FileAccess32::READ,
            share: FileShare32::ALL,
            disposition: FileDisposition32::OpenExisting,
            attributes: ATTR_NORMAL,
            directory: false,
            delete_on_close: false,
            overlapped: false,
            no_buffering: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileOpenResult32 {
    pub open: OpenFileId,
    pub file: FileId,
    pub existed: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileTimes32 {
    pub created: u64,
    pub accessed: u64,
    pub written: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo32 {
    pub id: FileId,
    pub attributes: u32,
    pub size: u64,
    pub times: FileTimes32,
    pub links: u32,
    pub delete_pending: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetRef32 {
    pub id: u64,
    pub length: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileReadRequest32 {
    pub epoch: u64,
    pub request: u64,
    pub file: FileId,
    pub revision: u64,
    pub asset: u64,
    pub offset: u64,
    pub length: u32,
}
#[derive(Debug)]
pub enum FileIo32<T> {
    Ready(T),
    NeedBytes(FileReadRequest32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSeek32 {
    Start,
    Current,
    End,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry32 {
    pub name: Vec<u16>,
    pub info: FileInfo32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind32 {
    File,
    Directory,
    Deleted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayEntry32 {
    pub epoch: u64,
    pub revision: u64,
    pub path: Vec<u16>,
    pub info: Option<FileInfo32>,
    pub kind: OverlayKind32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileVolume32 {
    pub serial: u32,
    pub readonly: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub max_component: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileIoOwner32 {
    GuestThread(u32),
    AsyncOperation(u64),
    HostExport,
}
