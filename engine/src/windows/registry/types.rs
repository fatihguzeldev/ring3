#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryLimits32 {
    pub keys: u32,
    pub values: u32,
    pub handles: u32,
    pub bytes: u32,
    pub changes: u32,
}
impl Default for RegistryLimits32 {
    fn default() -> Self {
        Self {
            keys: 4096,
            values: 16384,
            handles: 1024,
            bytes: 16 * 1024 * 1024,
            changes: 4096,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegistryConfig32 {
    pub limits: RegistryLimits32,
    pub separate_views: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RegistryError32 {
    FileNotFound = 2,
    BadNetworkPath = 53,
    UnsupportedType = 1630,
    DatatypeMismatch = 1629,
    AccessDenied = 5,
    InvalidHandle = 6,
    NotEnoughMemory = 8,
    InvalidData = 13,
    InvalidParameter = 87,
    NoAccess = 998,
    CallNotImplemented = 120,
    Busy = 170,
    TransferTooLong = 222,
    MoreData = 234,
    NoMoreItems = 259,
    KeyDeleted = 1018,
    ChildMustBeVolatile = 1021,
    KeyHasChildren = 1020,
    StaleReceipt = 0xffff_ff01,
    NotConfigured = 0xffff_ff02,
    AlreadyConfigured = 0xffff_ff03,
}
impl RegistryError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::StaleReceipt => 87,
            Self::NotConfigured | Self::AlreadyConfigured => 170,
            _ => self as u32,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryView32 {
    View32 = 32,
    View64 = 64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryKeyId32 {
    pub(crate) slot: u32,
    pub(crate) generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RegistryChangeKind32 {
    CreateKey = 1,
    SetValue = 2,
    DeleteValue = 3,
    DeleteTree = 4,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryChangeId32 {
    pub epoch: u64,
    pub sequence: u64,
    pub revision: u64,
}
