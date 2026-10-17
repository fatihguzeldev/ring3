#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommonControlKind32 {
    Progress,
    Status,
    Tab,
    Hotkey,
    Tooltip,
    Header,
    Toolbar,
    Trackbar,
    UpDown,
    ListView,
    Animate,
    TreeView,
}

impl CommonControlKind32 {
    pub(crate) const ALL: [Self; 12] = [
        Self::Progress,
        Self::Status,
        Self::Tab,
        Self::Hotkey,
        Self::Tooltip,
        Self::Header,
        Self::Toolbar,
        Self::Trackbar,
        Self::UpDown,
        Self::ListView,
        Self::Animate,
        Self::TreeView,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::TreeView => "SysTreeView32",
            Self::ListView => "SysListView32",
            Self::Progress => "msctls_progress32",
            Self::Status => "msctls_statusbar32",
            Self::Tab => "SysTabControl32",
            Self::Trackbar => "msctls_trackbar32",
            Self::UpDown => "msctls_updown32",
            Self::Animate => "SysAnimate32",
            Self::Header => "SysHeader32",
            Self::Hotkey => "msctls_hotkey32",
            Self::Tooltip => "tooltips_class32",
            Self::Toolbar => "ToolbarWindow32",
        }
    }

    pub(crate) fn flags(self) -> u32 {
        match self {
            Self::TreeView => 2,
            Self::ListView => 1,
            Self::Progress => 0x20,
            Self::Status => 4,
            Self::Tab => 8,
            Self::Trackbar => 4,
            Self::UpDown => 0x10,
            Self::Animate => 0x80,
            Self::Header => 1,
            Self::Hotkey => 0x40,
            Self::Tooltip => 0x0e,
            Self::Toolbar => 4,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommonControlState32 {
    Progress(super::progress::Progress32),
    Status(super::status::Status32),
    Tab(super::tab::Tab32),
    Trackbar(super::trackbar::Trackbar32),
    UpDown(super::updown::UpDown32),
    Animate(super::animate::Animate32),
    Header(super::header::Header32),
    Hotkey(super::hotkey::Hotkey32),
    Tooltip(super::tooltip::Tooltip32),
    Toolbar(super::toolbar::Toolbar32),
    ListView(super::listview::ListView32),
    TreeView(super::treeview::TreeView32),
}

impl CommonControlState32 {
    pub(crate) fn new(kind: CommonControlKind32) -> Self {
        match kind {
            CommonControlKind32::TreeView => Self::TreeView(Default::default()),
            CommonControlKind32::ListView => Self::ListView(Default::default()),
            CommonControlKind32::Progress => Self::Progress(Default::default()),
            CommonControlKind32::Status => Self::Status(Default::default()),
            CommonControlKind32::Tab => Self::Tab(Default::default()),
            CommonControlKind32::Trackbar => Self::Trackbar(Default::default()),
            CommonControlKind32::UpDown => Self::UpDown(Default::default()),
            CommonControlKind32::Animate => Self::Animate(Default::default()),
            CommonControlKind32::Header => Self::Header(Default::default()),
            CommonControlKind32::Hotkey => Self::Hotkey(Default::default()),
            CommonControlKind32::Tooltip => Self::Tooltip(Default::default()),
            CommonControlKind32::Toolbar => Self::Toolbar(Default::default()),
        }
    }
}
