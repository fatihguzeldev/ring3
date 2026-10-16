use crate::loader::{SystemExport32, SystemExportTarget32};
pub(crate) const BUILTIN_PROC_A: u32 = 0x001f_ff01;
pub(crate) const BUILTIN_PROC_W: u32 = 0x001f_ff02;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind32 {
    Button,
    Edit,
    Static,
    ListBox,
    ScrollBar,
    ComboBox,
}
impl ControlKind32 {
    pub const STANDARD: [Self; 6] = [
        Self::Button,
        Self::Edit,
        Self::Static,
        Self::ListBox,
        Self::ScrollBar,
        Self::ComboBox,
    ];
    pub fn from_resource_ordinal(ordinal: u16) -> Option<Self> {
        Self::STANDARD.into_iter().find(|kind| kind.resource_ordinal() == ordinal)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Button => "Button",
            Self::Edit => "Edit",
            Self::Static => "Static",
            Self::ListBox => "ListBox",
            Self::ComboBox => "ComboBox",
            Self::ScrollBar => "ScrollBar",
        }
    }
    pub fn resource_ordinal(self) -> u16 {
        0x80 + self as u16
    }
}
pub(crate) fn builtin_exports() -> impl Iterator<Item = SystemExport32> {
    [BUILTIN_PROC_A, BUILTIN_PROC_W]
        .into_iter()
        .map(|id| SystemExport32 {
            name: None,
            ordinal: None,
            target: SystemExportTarget32::PrivateGate(id),
        })
}
