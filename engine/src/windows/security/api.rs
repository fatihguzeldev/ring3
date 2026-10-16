#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum SecurityApi32 {
    GetFileSecurityA = 0x0019_0001,
    GetFileSecurityW,
    SetFileSecurityA,
    SetFileSecurityW,
    IsValidSecurityDescriptor,
    GetSecurityDescriptorLength,
    GetSecurityInfo,
    SetSecurityInfo,
    GetNamedSecurityInfoA,
    GetNamedSecurityInfoW,
    SetNamedSecurityInfoA,
    SetNamedSecurityInfoW,
}
impl SecurityApi32 {
    pub(crate) const ALL: [Self; 12] = [
        Self::GetFileSecurityA,
        Self::GetFileSecurityW,
        Self::SetFileSecurityA,
        Self::SetFileSecurityW,
        Self::IsValidSecurityDescriptor,
        Self::GetSecurityDescriptorLength,
        Self::GetSecurityInfo,
        Self::SetSecurityInfo,
        Self::GetNamedSecurityInfoA,
        Self::GetNamedSecurityInfoW,
        Self::SetNamedSecurityInfoA,
        Self::SetNamedSecurityInfoW,
    ];
    pub(crate) fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0019_0001)? as usize)
            .copied()
    }
    pub(crate) fn id(self) -> u32 {
        self as u32
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::GetFileSecurityA => "GetFileSecurityA",
            Self::GetFileSecurityW => "GetFileSecurityW",
            Self::SetFileSecurityA => "SetFileSecurityA",
            Self::SetFileSecurityW => "SetFileSecurityW",
            Self::IsValidSecurityDescriptor => "IsValidSecurityDescriptor",
            Self::GetSecurityDescriptorLength => "GetSecurityDescriptorLength",
            Self::GetSecurityInfo => "GetSecurityInfo",
            Self::SetSecurityInfo => "SetSecurityInfo",
            Self::GetNamedSecurityInfoA => "GetNamedSecurityInfoA",
            Self::GetNamedSecurityInfoW => "GetNamedSecurityInfoW",
            Self::SetNamedSecurityInfoA => "SetNamedSecurityInfoA",
            Self::SetNamedSecurityInfoW => "SetNamedSecurityInfoW",
        }
    }
    pub(crate) fn stack_words(self) -> u32 {
        match self {
            Self::GetFileSecurityA | Self::GetFileSecurityW => 5,
            Self::SetFileSecurityA | Self::SetFileSecurityW => 3,
            Self::IsValidSecurityDescriptor | Self::GetSecurityDescriptorLength => 1,
            Self::GetSecurityInfo | Self::GetNamedSecurityInfoA | Self::GetNamedSecurityInfoW => 8,
            Self::SetSecurityInfo | Self::SetNamedSecurityInfoA | Self::SetNamedSecurityInfoW => 7,
        }
    }
    pub(crate) fn returns_status(self) -> bool {
        self.id() >= Self::GetSecurityInfo.id()
    }
}
