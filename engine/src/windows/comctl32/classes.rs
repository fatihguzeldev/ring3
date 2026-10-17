#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommonControlKind32 {
    Progress,
    Status,
    Tab,
    Hotkey,
    Tooltip,
}

impl CommonControlKind32 {
    pub(crate) const ALL: [Self; 5] = [
        Self::Progress,
        Self::Status,
        Self::Tab,
        Self::Hotkey,
        Self::Tooltip,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Progress => "msctls_progress32",
            Self::Status => "msctls_statusbar32",
            Self::Tab => "SysTabControl32",
            Self::Hotkey => "msctls_hotkey32",
            Self::Tooltip => "tooltips_class32",
        }
    }

    pub(crate) fn flags(self) -> u32 {
        match self {
            Self::Progress => 0x20,
            Self::Status => 4,
            Self::Tab => 8,
            Self::Hotkey => 0x40,
            Self::Tooltip => 0x0e,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommonControlState32 {
    Progress(super::progress::Progress32),
    Status(super::status::Status32),
    Tab(super::tab::Tab32),
    Hotkey(super::hotkey::Hotkey32),
    Tooltip(super::tooltip::Tooltip32),
}

impl CommonControlState32 {
    pub(crate) fn new(kind: CommonControlKind32) -> Self {
        match kind {
            CommonControlKind32::Progress => Self::Progress(Default::default()),
            CommonControlKind32::Status => Self::Status(Default::default()),
            CommonControlKind32::Tab => Self::Tab(Default::default()),
            CommonControlKind32::Hotkey => Self::Hotkey(Default::default()),
            CommonControlKind32::Tooltip => Self::Tooltip(Default::default()),
        }
    }
}
