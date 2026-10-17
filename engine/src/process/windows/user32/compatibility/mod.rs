mod controls;
mod geometry;
mod system;
mod text;
mod windows;

use super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    windows::user32::{User32Api32, User32Error32},
};

#[derive(Default)]
pub(super) struct Compatibility32 {
    properties: Vec<(u32, Vec<u16>, u32)>,
    scroll: Vec<(u32, u32, Scroll32)>,
    locked: Option<u32>,
    pub(super) desktop: Option<crate::windows::window::WindowHandle32>,
}

#[derive(Clone, Copy, Default)]
struct Scroll32 {
    min: i32,
    max: i32,
    page: u32,
    pos: i32,
    track: i32,
}

impl User32Context32 {
    pub(super) fn compatibility_call(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        match api {
            SetRect | SetRectEmpty | CopyRect | EqualRect | IsRectEmpty | InflateRect
            | OffsetRect | IntersectRect | UnionRect | PtInRect | AdjustWindowRectEx
            | MapWindowPoints => self.geometry_call(api, args, memory),
            CharNextA | CharUpperA | CharToOemA | OemToCharA | WvsprintfA | WsprintfA => {
                self.text_utility_call(api, args, memory, context)
            }
            GetClassInfoA
            | GetClassLongA
            | GetClassNameA
            | GetWindow
            | GetTopWindow
            | GetParent
            | SetParent
            | IsChild
            | GetWindowThreadProcessId
            | FindWindowA
            | GetPropA
            | SetPropA
            | RemovePropA
            | IsWindowVisible
            | IsWindowEnabled
            | IsIconic
            | IsZoomed
            | GetWindowPlacement
            | SetWindowPlacement
            | SetActiveWindow
            | SetForegroundWindow
            | BringWindowToTop
            | EnableWindow
            | GetLastActivePopup
            | ShowOwnedPopups
            | WindowFromPoint
            | EnumChildWindows => self.window_utility_call(api, args, memory, context),
            GetDlgItemTextA | SetDlgItemTextA | SendDlgItemMessageA | GetDlgItemInt
            | SetDlgItemInt | CheckDlgButton | CheckRadioButton | IsDlgButtonChecked
            | GetNextDlgTabItem | GetNextDlgGroupItem | IsDialogMessageA | GetScrollInfo
            | SetScrollInfo | GetScrollRange | SetScrollRange | GetScrollPos | SetScrollPos
            | ShowScrollBar => self.control_utility_call(api, args, memory, context),
            _ => self.system_utility_call(api, args, memory, context),
        }
    }
}

impl Compatibility32 {
    pub(super) fn retire_window(&mut self, window: u32) {
        self.properties.retain(|(handle, _, _)| *handle != window);
        self.scroll.retain(|(handle, _, _)| *handle != window);
    }
}
