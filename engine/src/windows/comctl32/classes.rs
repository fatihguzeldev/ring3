#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommonControlKind32 {
    Progress,
    Status,
    Tab,
}

impl CommonControlKind32 {
    pub(crate) const ALL: [Self; 3] = [Self::Progress, Self::Status, Self::Tab];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Progress => "msctls_progress32",
            Self::Status => "msctls_statusbar32",
            Self::Tab => "SysTabControl32",
        }
    }

    pub(crate) fn flags(self) -> u32 {
        match self {
            Self::Progress => 0x20,
            Self::Status => 4,
            Self::Tab => 8,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommonControlState32 {
    Progress(super::progress::Progress32),
    Status(super::status::Status32),
    Tab(super::tab::Tab32),
}

impl CommonControlState32 {
    pub(crate) fn new(kind: CommonControlKind32) -> Self {
        match kind {
            CommonControlKind32::Progress => Self::Progress(Default::default()),
            CommonControlKind32::Status => Self::Status(Default::default()),
            CommonControlKind32::Tab => Self::Tab(Default::default()),
        }
    }
}
